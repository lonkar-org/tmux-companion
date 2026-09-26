//! Projects: one tmux session each, with the windows a layout asks for.
//!
//! Why sessions rather than more windows. With every project as two windows in
//! one session, "which project" and "which tool" share a single index, so at
//! five projects you are hopping through ten unrelated windows to reach the
//! other half of the one you are in. A session per project splits that into two
//! axes, and the toggle moves between tools without touching the project.
//!
//! The list is sessions first, most recently attached first, then whatever the
//! directory source knows -- zoxide by default, and [`crate::dirsource`] for
//! the rest. A directory the source has never seen cannot be picked from the
//! list, because the list is visits; typing a path that matches nothing opens
//! it anyway and records the first visit.

use std::collections::HashMap;

/// Where a row came from, which decides what picking it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A live tmux session: picking switches to it.
    Session,
    /// A directory the source knows: picking opens or creates its session.
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
    /// Seconds since anything happened in the session, for a live session no
    /// client is attached to. `None` for a directory and for a session
    /// somebody is in, however quiet.
    pub idle: Option<u64>,
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

/// The `list-sessions -F` format [`sessions_from`] reads: last attached, name,
/// path, theme colour, attached clients, and last activity in unix seconds.
const SESSIONS_FORMAT: &str = "#{session_last_attached}\t#{session_name}\t#{session_path}\t#{@theme-color-main-1}\t#{session_attached}\t#{session_activity}";

/// Parse `tmux list-sessions` output into rows, most recently attached first.
///
/// tmux lists alphabetically and this wants recency, because a picker whose
/// first row is the session you are in and whose second is the one you were in
/// before makes "go back" a keypress rather than a search.
///
/// `now` is unix seconds and is what the idle age is measured against; it is
/// an argument so the parse can be tested against a fixed clock. The two
/// fields behind it are optional in the listing, so a row without them is a
/// session whose idleness is simply unknown rather than a line dropped.
pub fn sessions_from(listing: &str, now: u64) -> Vec<Row> {
    let mut with_time: Vec<(i64, Row)> = listing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut f = line.split('\t');
            let last_attached: i64 = f.next()?.trim().parse().unwrap_or(0);
            let label = f.next()?.to_string();
            let path = f.next()?.to_string();
            let colour = f.next().filter(|c| !c.is_empty()).map(str::to_string);
            let attached: Option<u64> = f.next().and_then(|v| v.trim().parse().ok());
            let activity: Option<u64> = f.next().and_then(|v| v.trim().parse().ok());
            let idle = match (attached, activity) {
                (Some(attached), Some(activity)) => {
                    crate::sessions::idle::idle_for(attached, activity, now)
                }
                _ => None,
            };
            Some((
                last_attached,
                Row {
                    kind: Kind::Session,
                    label,
                    path,
                    colour,
                    idle,
                },
            ))
        })
        .collect();
    with_time.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    with_time.into_iter().map(|(_, r)| r).collect()
}

/// The `idle 5d` a picker row carries, or nothing.
///
/// Only from a day up. A session left ten minutes ago is one somebody is
/// between, and a label on it would make every detached session look stale,
/// which is the opposite of the point: the label is there so the handful
/// worth closing stand out from the rest.
pub fn idle_column(row: &Row) -> Option<String> {
    row.idle
        .filter(|secs| *secs >= crate::sessions::idle::DAY)
        .map(|secs| format!("idle {}", crate::sessions::idle::age(secs)))
}

/// Turn a directory source's lines into rows, colouring each from the project
/// map.
pub fn directories_from(
    listing: &str,
    project_theme: &HashMap<String, String>,
    theme_colour: &HashMap<String, String>,
    default_theme: &str,
) -> Vec<Row> {
    // The same six the session-created hook would choose from, so a directory
    // shows the colour its session is about to get.
    let by_name: Vec<&str> = if default_theme == crate::theme::BY_NAME {
        crate::theme::BASE_THEMES
            .iter()
            .map(|(stem, _, _)| *stem)
            .filter(|stem| theme_colour.contains_key(*stem))
            .collect()
    } else {
        Vec::new()
    };
    listing
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|path| {
            let label = session_name(path);
            let colour = project_theme
                .get(&label)
                .and_then(|theme| theme_colour.get(theme))
                .or_else(|| {
                    crate::theme::theme_by_name(&label, &by_name)
                        .and_then(|stem| theme_colour.get(&stem))
                })
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
                idle: None,
            }
        })
        .collect()
}

/// Sessions then directories, with any path already listed dropped.
///
/// A project that is open is a session row and a directory row at once, and the
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
/// The only way into a directory the source has never seen: it lists visits, and a
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

/// Every row the picker should show: sessions first, then the directory source.
pub async fn collect(config: &crate::config::Config, home: &str) -> Vec<Row> {
    let sessions = sessions_from(&tmux_sessions().await, crate::sessions::idle::now());
    let source = crate::dirsource::DirsSource::from_config(&config.project);
    if source == crate::dirsource::DirsSource::None {
        return sessions;
    }
    let map = std::fs::read_to_string(project_map_path(home))
        .map(|t| parse_project_map(&t))
        .unwrap_or_default();
    let colours = theme_colours(&read_theme_files(home));
    let listing = source.list(home).await.join("\n");
    merge(
        sessions,
        directories_from(&listing, &map, &colours, &config.theme.default),
    )
}

/// `tmux list-sessions`, with the fields the rows need.
async fn tmux_sessions() -> String {
    let out = tokio::process::Command::new("tmux")
        .args(["list-sessions", "-F", SESSIONS_FORMAT])
        .output()
        .await;
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        // No server running yet is the ordinary case for the first project of
        // the day, not an error.
        Err(_) => String::new(),
    }
}

/// The directories the configured source knows, most relevant first.
///
/// `new-window` wants the paths and nothing else, where the project picker
/// wants them merged with live sessions and coloured, so the shared part stops
/// here.
pub async fn source_dirs(config: &crate::config::Config, home: &str) -> Vec<String> {
    crate::dirsource::DirsSource::from_config(&config.project)
        .list(home)
        .await
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

/// Tell the directory source this directory was visited, if it takes telling.
pub async fn record_visit(config: &crate::config::Config, dir: &str) {
    let source = crate::dirsource::DirsSource::from_config(&config.project);
    crate::dirsource::record_visit(&source, &config.project, dir).await;
}

// ── building a session ───────────────────────────────────────────────────────

/// Everything `session_commands` needs, as one struct so the signature does not
/// grow past what clippy will accept.
#[derive(Debug, Clone, Copy)]
pub struct SessionSpec<'a> {
    /// The tmux session name.
    pub name: &'a str,
    /// The project directory, which is where every window starts.
    pub path: &'a str,
    /// Home, for expanding a `~` in a pane's `cwd`.
    pub home: &'a str,
    /// The server's `pane-base-index`, read once rather than assumed.
    ///
    /// Targeting `window.0` on a config that sets `pane-base-index 1` hits
    /// nothing, and tmux reports that as a failed command rather than an error
    /// anybody sees, so the session would come up with the commands silently
    /// missing.
    pub pane_base: usize,
    /// The windows to build, first one selected at the end.
    pub windows: &'a [crate::config::LayoutWindow],
}

/// `~/x` against a home directory, anything else unchanged.
fn expand_home(dir: &str, home: &str) -> String {
    match dir.strip_prefix("~/") {
        Some(rest) => format!("{home}/{rest}"),
        None if dir == "~" => home.to_string(),
        None => dir.to_string(),
    }
}

/// The tmux commands that build a session, in the order they have to run.
///
/// Pure on purpose: building a session is a dozen ordered calls whose order is
/// the entire feature, and the only way to test an order is to be able to look
/// at it without a tmux server in the room.
///
/// The order is windows, then panes, then geometry, then the commands. Commands
/// last matters: sent before the layout is applied, a full-screen program draws
/// itself at the pre-split size and repaints, which looks broken on every
/// session start.
///
/// A window with no `pane` table emits exactly what this function emitted
/// before panes existed, down to the window-level `send-keys` target, so an
/// existing config builds the same session it always did.
pub fn session_commands(spec: &SessionSpec) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let SessionSpec {
        name,
        path,
        home,
        pane_base,
        windows,
    } = *spec;

    let Some((first, rest)) = windows.split_first() else {
        // A layout with no windows is a plain shell, which is what somebody
        // asking for no windows asked for.
        return vec![args(&["new-session", "-d", "-s", name, "-c", path])];
    };

    out.push(args(&[
        "new-session",
        "-d",
        "-s",
        name,
        "-c",
        path,
        "-n",
        &first.name,
    ]));
    // Which project this session is, kept against a rename and against panes
    // that wander somewhere else, so `project save` on this session writes to
    // the right file.
    out.push(args(&[
        "set-option",
        "-t",
        &format!("={name}"),
        "@tmux-companion-project",
        path,
    ]));
    for w in rest {
        out.push(args(&[
            "new-window",
            "-d",
            "-t",
            &format!("={name}:"),
            "-c",
            path,
            "-n",
            &w.name,
        ]));
    }

    for w in windows {
        let target = format!("={name}:{}", w.name);
        let count = w.pane_count();

        // Each split targets the pane made by the previous one, so the panes
        // end up in the order the config lists them. Splitting the first pane
        // every time would interleave them, because tmux inserts a new pane
        // directly after the one it split.
        for i in 1..count {
            // Reflow before every split past the first: repeated halving runs
            // out of rows or columns around the fourth pane, and tmux answers
            // "no space for new pane" rather than making room.
            if i > 1 {
                out.push(args(&["select-layout", "-t", &target, "tiled"]));
            }
            let cwd = w.pane[i]
                .cwd
                .as_deref()
                .map(|d| expand_home(d, home))
                .unwrap_or_else(|| path.to_string());
            out.push(args(&[
                "split-window",
                "-d",
                "-t",
                &format!("{target}.{}", pane_base + i - 1),
                "-c",
                &cwd,
            ]));
        }

        if count > 1 {
            if let (Some(option), Some(size)) = (w.main_size_option(), w.main_size.as_deref()) {
                out.push(args(&["set-window-option", "-t", &target, option, size]));
            }
            // Tiled when nothing is asked for, because the shape left by the
            // splits is an accident of the order they ran in.
            let layout = w.layout.as_deref().unwrap_or("tiled");
            out.push(args(&["select-layout", "-t", &target, layout]));
        } else if let Some(layout) = w.layout.as_deref() {
            out.push(args(&["select-layout", "-t", &target, layout]));
        }
    }

    for w in windows {
        if w.hold_name {
            let target = format!("={name}:{}", w.name);
            out.push(args(&[
                "set-window-option",
                "-t",
                &target,
                "automatic-rename",
                "off",
            ]));
            out.push(args(&[
                "set-window-option",
                "-t",
                &target,
                "allow-rename",
                "off",
            ]));
        }
    }

    for w in windows {
        let target = format!("={name}:{}", w.name);
        if w.pane.is_empty() {
            if !w.command.is_empty() {
                out.push(args(&["send-keys", "-t", &target, &w.command, "C-m"]));
            }
            continue;
        }
        for (i, p) in w.pane.iter().enumerate() {
            if !p.command.is_empty() {
                out.push(args(&[
                    "send-keys",
                    "-t",
                    &format!("{target}.{}", pane_base + i),
                    &p.command,
                    "C-m",
                ]));
            }
        }
        let focused = w.focused_pane();
        if focused != 0 {
            out.push(args(&[
                "select-pane",
                "-t",
                &format!("{target}.{}", pane_base + focused),
            ]));
        }
    }

    out.push(args(&[
        "select-window",
        "-t",
        &format!("={name}:{}", first.name),
    ]));
    out
}

/// One command, owned, so the caller can hand it to a process builder.
fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
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
        let rows = sessions_from(listing, 0);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["zulu", "mike", "alpha"]);
    }

    #[test]
    fn a_session_with_no_theme_has_no_colour_rather_than_a_wrong_one() {
        let rows = sessions_from("200\tmike\t/m\t\n", 0);
        assert_eq!(rows[0].colour, None);
    }

    #[test]
    fn a_detached_session_carries_its_age_and_an_attached_one_does_not() {
        use crate::sessions::idle::DAY;
        let now = 1_700_000_000;
        let listing = format!(
            "100\tstale\t/s\tcolour60\t0\t{}\n200\tbusy\t/b\t\t1\t{}\n300\told\t/o\t\t\n",
            now - 5 * DAY,
            now - 5 * DAY,
        );
        let rows = sessions_from(&listing, now);
        let by_name = |n: &str| rows.iter().find(|r| r.label == n).unwrap();
        assert_eq!(by_name("stale").idle, Some(5 * DAY));
        // Somebody is in it, however long it has sat.
        assert_eq!(by_name("busy").idle, None);
        // A listing from before the two fields existed still parses.
        assert_eq!(by_name("old").idle, None);
    }

    #[test]
    fn the_idle_column_appears_from_a_day_up_and_not_before() {
        use crate::sessions::idle::DAY;
        let row = |idle: Option<u64>| Row {
            kind: Kind::Session,
            label: "x".into(),
            path: "/x".into(),
            colour: None,
            idle,
        };
        // Ten minutes ago is a session somebody is between, not a stale one.
        assert_eq!(idle_column(&row(Some(600))), None);
        assert_eq!(idle_column(&row(Some(DAY - 1))), None);
        assert_eq!(idle_column(&row(Some(DAY))).as_deref(), Some("idle 1d"));
        assert_eq!(
            idle_column(&row(Some(5 * DAY + 7))).as_deref(),
            Some("idle 5d")
        );
        assert_eq!(idle_column(&row(None)), None);
    }

    #[test]
    fn directories_take_their_colour_through_the_project_map() {
        let project_theme = HashMap::from([("mysetup".to_string(), "indigo".to_string())]);
        let theme_colour = HashMap::from([("indigo".to_string(), "colour60".to_string())]);
        let rows = directories_from(
            "/Users/y/git-repos/mysetup\n",
            &project_theme,
            &theme_colour,
            "ink",
        );
        assert_eq!(rows[0].colour.as_deref(), Some("colour60"));
        assert_eq!(rows[0].label, "mysetup");
        assert_eq!(rows[0].kind, Kind::Directory);
    }

    #[test]
    fn a_by_name_default_colours_a_directory_the_way_its_session_will_be() {
        // The hook picks from the bundled six by name; the picker row shows
        // that colour before the session exists, and the map still wins.
        let theme_colour: HashMap<String, String> = crate::theme::BASE_THEMES
            .iter()
            .map(|(stem, _, index)| (stem.to_string(), format!("colour{index}")))
            .collect();
        let rows = directories_from("/w/api\n", &HashMap::new(), &theme_colour, "by-name");
        let expected =
            crate::theme::theme_by_name("api", &["ember", "pine", "slate", "plum", "sand", "ink"])
                .unwrap();
        assert_eq!(
            rows[0].colour.as_deref(),
            theme_colour.get(&expected).map(String::as_str)
        );

        let mapped = HashMap::from([("api".to_string(), "ink".to_string())]);
        let rows = directories_from("/w/api\n", &mapped, &theme_colour, "by-name");
        assert_eq!(rows[0].colour.as_deref(), Some("colour104"));

        // A plain default names nothing for an unmapped directory.
        let rows = directories_from("/w/api\n", &HashMap::new(), &theme_colour, "ink");
        assert_eq!(rows[0].colour, None);
    }

    #[test]
    fn a_directory_the_map_does_not_know_has_no_colour() {
        let rows = directories_from("/tmp/unknown\n", &HashMap::new(), &HashMap::new(), "ink");
        assert_eq!(rows[0].colour, None);
    }

    #[test]
    fn an_open_project_appears_once_as_its_session() {
        // The session row knows the session's real name, which a path does not
        // always give: session `y` sits at the home directory, and deriving a
        // name from that opened a new session called `yogesh` the first time.
        let sessions = sessions_from("100\ty\t/Users/yogesh\t\n", 0);
        let dirs = directories_from("/Users/yogesh\n", &HashMap::new(), &HashMap::new(), "ink");
        let merged = merge(sessions, dirs);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, Kind::Session);
        assert_eq!(merged[0].label, "y");
    }

    #[test]
    fn directories_that_are_not_open_survive_the_merge() {
        let sessions = sessions_from("100\tone\t/one\t\n", 0);
        let dirs = directories_from("/two\n/three\n", &HashMap::new(), &HashMap::new(), "ink");
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

#[cfg(test)]
mod session_building {
    use super::*;
    use crate::config::{LayoutPane, LayoutWindow};

    fn win(name: &str, command: &str) -> LayoutWindow {
        LayoutWindow {
            name: name.to_string(),
            command: command.to_string(),
            hold_name: true,
            layout: None,
            main_size: None,
            pane: Vec::new(),
        }
    }

    fn pane(command: &str) -> LayoutPane {
        LayoutPane {
            command: command.to_string(),
            ..Default::default()
        }
    }

    fn build(windows: &[LayoutWindow], pane_base: usize) -> Vec<Vec<String>> {
        session_commands(&SessionSpec {
            name: "proj",
            path: "/w/proj",
            home: "/home/me",
            pane_base,
            windows,
        })
    }

    fn joined(cmds: &[Vec<String>]) -> Vec<String> {
        cmds.iter().map(|c| c.join(" ")).collect()
    }

    #[test]
    fn the_default_layout_builds_what_it_built_before_panes_existed() {
        // The pinned sequence. A window with no `pane` table has to emit the
        // same calls in the same order as the hand-written version this
        // replaced, including the window-level `send-keys` target. The one
        // addition is the `@tmux-companion-project` option, which is what lets
        // `project save` find the right file from a renamed session.
        // The editor-and-agent pair that was the shipped default when this
        // was pinned, written out now that the default is a plain shell.
        let c = crate::config::parse(
            "[[layout]]\nname = \"default\"\n\n[[layout.window]]\nname = \"edit\"\ncommand = \"nvim\"\nhold_name = true\n\n[[layout.window]]\nname = \"ai\"\ncommand = \"claude\"\nhold_name = true\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap();
        let l = c
            .layout_for("/w/proj", "/home/me")
            .expect("the layout just written");
        assert_eq!(
            joined(&build(&l.window, 0)),
            vec![
                "new-session -d -s proj -c /w/proj -n edit",
                "set-option -t =proj @tmux-companion-project /w/proj",
                "new-window -d -t =proj: -c /w/proj -n ai",
                "set-window-option -t =proj:edit automatic-rename off",
                "set-window-option -t =proj:edit allow-rename off",
                "set-window-option -t =proj:ai automatic-rename off",
                "set-window-option -t =proj:ai allow-rename off",
                "send-keys -t =proj:edit nvim C-m",
                "send-keys -t =proj:ai claude C-m",
                "select-window -t =proj:edit",
            ]
        );
    }

    #[test]
    fn no_windows_is_a_plain_shell() {
        assert_eq!(
            joined(&build(&[], 0)),
            vec!["new-session -d -s proj -c /w/proj"]
        );
    }

    #[test]
    fn a_window_with_no_command_sends_no_keys() {
        let w = win("shell", "");
        let out = joined(&build(&[w], 0));
        assert!(!out.iter().any(|c| c.starts_with("send-keys")), "{out:?}");
    }

    #[test]
    fn each_split_targets_the_pane_the_last_one_made() {
        // Splitting pane 0 every time interleaves the panes, because tmux
        // inserts a new pane directly after the one it split, so pane 3 in the
        // config would land in the middle of the window.
        let mut w = win("edit", "");
        w.pane = vec![pane("a"), pane("b"), pane("c")];
        let out = joined(&build(&[w], 0));
        let splits: Vec<&String> = out
            .iter()
            .filter(|c| c.starts_with("split-window"))
            .collect();
        assert_eq!(
            splits,
            vec![
                "split-window -d -t =proj:edit.0 -c /w/proj",
                "split-window -d -t =proj:edit.1 -c /w/proj",
            ]
        );
    }

    #[test]
    fn a_third_pane_gets_a_reflow_before_it_so_there_is_room() {
        let mut w = win("edit", "");
        w.pane = vec![pane(""), pane(""), pane("")];
        let out = joined(&build(&[w], 0));
        let i = out
            .iter()
            .position(|c| c == "select-layout -t =proj:edit tiled");
        let second = out
            .iter()
            .position(|c| c == "split-window -d -t =proj:edit.1 -c /w/proj")
            .unwrap();
        assert!(i.is_some_and(|i| i < second), "{out:?}");
    }

    #[test]
    fn two_panes_need_no_reflow() {
        let mut w = win("edit", "");
        w.pane = vec![pane(""), pane("")];
        let out = joined(&build(&[w], 0));
        assert_eq!(
            out.iter().filter(|c| c.starts_with("split-window")).count(),
            1
        );
        // One select-layout, the final one, not an intermediate reflow.
        assert_eq!(
            out.iter()
                .filter(|c| c.starts_with("select-layout"))
                .count(),
            1
        );
    }

    #[test]
    fn panes_with_no_layout_get_tiled_rather_than_whatever_the_splits_left() {
        let mut w = win("edit", "");
        w.pane = vec![pane(""), pane("")];
        let out = joined(&build(&[w], 0));
        assert!(
            out.contains(&"select-layout -t =proj:edit tiled".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn a_raw_tmux_layout_string_is_passed_through_unparsed() {
        let mut w = win("edit", "");
        w.layout = Some("bb62,80x24,0,0{40x24,0,0,1,39x24,41,0,2}".to_string());
        w.pane = vec![pane(""), pane("")];
        let out = joined(&build(&[w], 0));
        assert!(
            out.contains(
                &"select-layout -t =proj:edit bb62,80x24,0,0{40x24,0,0,1,39x24,41,0,2}".to_string()
            ),
            "{out:?}"
        );
    }

    #[test]
    fn main_size_is_a_width_for_main_vertical_and_a_height_for_main_horizontal() {
        let mut w = win("edit", "");
        w.pane = vec![pane(""), pane("")];
        w.main_size = Some("60%".to_string());

        w.layout = Some("main-vertical".to_string());
        assert!(
            joined(&build(&[w.clone()], 0))
                .contains(&"set-window-option -t =proj:edit main-pane-width 60%".to_string())
        );

        w.layout = Some("main-horizontal".to_string());
        assert!(
            joined(&build(&[w.clone()], 0))
                .contains(&"set-window-option -t =proj:edit main-pane-height 60%".to_string())
        );

        // tiled reads neither, so setting one would be a call that does
        // nothing and a reader wondering why it is there.
        w.layout = Some("tiled".to_string());
        assert!(
            !joined(&build(&[w], 0))
                .iter()
                .any(|c| c.contains("main-pane"))
        );
    }

    #[test]
    fn main_size_is_set_before_the_layout_that_reads_it() {
        let mut w = win("edit", "");
        w.pane = vec![pane(""), pane("")];
        w.layout = Some("main-vertical".to_string());
        w.main_size = Some("60%".to_string());
        let out = joined(&build(&[w], 0));
        let size = out
            .iter()
            .position(|c| c.contains("main-pane-width"))
            .unwrap();
        let layout = out
            .iter()
            .position(|c| c == "select-layout -t =proj:edit main-vertical")
            .unwrap();
        assert!(size < layout, "{out:?}");
    }

    #[test]
    fn commands_are_sent_after_the_layout_is_applied() {
        // A full-screen program started before the splits draws itself at the
        // wrong size and repaints, which looks broken on every session start.
        let mut w = win("edit", "");
        w.pane = vec![pane("nvim"), pane("claude")];
        let out = joined(&build(&[w], 0));
        let layout = out
            .iter()
            .position(|c| c.starts_with("select-layout"))
            .unwrap();
        let first_key = out.iter().position(|c| c.starts_with("send-keys")).unwrap();
        assert!(layout < first_key, "{out:?}");
    }

    #[test]
    fn each_pane_gets_its_own_command_at_its_own_index() {
        let mut w = win("edit", "");
        w.pane = vec![pane("nvim"), pane(""), pane("claude")];
        let out = joined(&build(&[w], 0));
        let keys: Vec<&String> = out.iter().filter(|c| c.starts_with("send-keys")).collect();
        assert_eq!(
            keys,
            vec![
                "send-keys -t =proj:edit.0 nvim C-m",
                "send-keys -t =proj:edit.2 claude C-m",
            ]
        );
    }

    #[test]
    fn pane_base_index_one_shifts_every_pane_target() {
        let mut w = win("edit", "");
        w.pane = vec![pane("nvim"), pane("claude")];
        let out = joined(&build(&[w], 1));
        assert!(
            out.contains(&"split-window -d -t =proj:edit.1 -c /w/proj".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"send-keys -t =proj:edit.1 nvim C-m".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"send-keys -t =proj:edit.2 claude C-m".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn focus_selects_that_pane_and_the_first_pane_needs_no_call() {
        let mut w = win("edit", "");
        w.pane = vec![pane("nvim"), pane("claude")];
        assert!(
            !joined(&build(&[w.clone()], 0))
                .iter()
                .any(|c| c.starts_with("select-pane"))
        );

        w.pane[1].focus = true;
        assert!(joined(&build(&[w], 0)).contains(&"select-pane -t =proj:edit.1".to_string()));
    }

    #[test]
    fn two_focused_panes_take_the_first_rather_than_erroring() {
        let mut w = win("edit", "");
        w.pane = vec![pane(""), pane(""), pane("")];
        w.pane[1].focus = true;
        w.pane[2].focus = true;
        assert!(joined(&build(&[w], 0)).contains(&"select-pane -t =proj:edit.1".to_string()));
    }

    #[test]
    fn a_pane_cwd_expands_a_tilde_and_otherwise_inherits_the_project() {
        let mut w = win("edit", "");
        w.pane = vec![pane(""), pane(""), pane("")];
        w.pane[1].cwd = Some("~/src".to_string());
        w.pane[2].cwd = Some("/etc".to_string());
        let out = joined(&build(&[w], 0));
        assert!(
            out.contains(&"split-window -d -t =proj:edit.0 -c /home/me/src".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"split-window -d -t =proj:edit.1 -c /etc".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn a_single_pane_window_still_honours_an_explicit_layout() {
        // One pane and a preset is a way of saying "leave it alone but set the
        // option", and dropping it silently would be surprising.
        let mut w = win("edit", "nvim");
        w.layout = Some("even-horizontal".to_string());
        let out = joined(&build(&[w], 0));
        assert!(
            out.contains(&"select-layout -t =proj:edit even-horizontal".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn hold_name_off_leaves_the_rename_options_alone() {
        let mut w = win("edit", "nvim");
        w.hold_name = false;
        let out = joined(&build(&[w], 0));
        assert!(
            !out.iter().any(|c| c.contains("automatic-rename")),
            "{out:?}"
        );
    }

    #[test]
    fn the_first_window_is_the_one_selected() {
        let out = joined(&build(&[win("edit", ""), win("ai", "")], 0));
        assert_eq!(out.last().unwrap(), "select-window -t =proj:edit");
    }

    #[test]
    fn expand_home_leaves_a_bare_path_alone() {
        assert_eq!(expand_home("~/src", "/home/me"), "/home/me/src");
        assert_eq!(expand_home("~", "/home/me"), "/home/me");
        assert_eq!(expand_home("/etc", "/home/me"), "/etc");
        assert_eq!(expand_home("relative", "/home/me"), "relative");
    }
}

// ── a window, rather than a session ──────────────────────────────────────────

/// What `new-window` should do once somebody has chosen or typed something.
///
/// Separated from the picker and from tmux so the resolution order is testable,
/// which matters because it has three fallbacks and the interesting one is the
/// case where nothing was typed at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowTarget {
    /// Open a window here.
    Open(String),
    /// Somebody typed a path that is not a directory.
    NoSuchDirectory(String),
    /// Nothing to do.
    Cancelled,
}

/// Resolve a picker outcome into the directory a new window should open in.
///
/// `prefill` is the shortened form of the pane's own directory, which is what
/// the query starts on. Enter with the query untouched means "another window
/// here", and the pane's real path is used rather than the shortened one,
/// because shortening throws away characters that cannot be put back: on this
/// machine `~/l/yogesh.lonkar.org` has lost the `lonkar-org` it came from.
pub fn window_target(
    outcome: &crate::picker::Outcome,
    rows: &[String],
    prefill: &str,
    pane_dir: &str,
    home: &str,
) -> WindowTarget {
    match outcome {
        crate::picker::Outcome::Cancelled => WindowTarget::Cancelled,
        crate::picker::Outcome::Chosen(i) => match rows.get(*i) {
            Some(path) => WindowTarget::Open(path.clone()),
            None => WindowTarget::Cancelled,
        },
        crate::picker::Outcome::Typed(query) => {
            let q = query.trim();
            if q.is_empty() {
                return WindowTarget::Cancelled;
            }
            if q == prefill {
                return WindowTarget::Open(pane_dir.to_string());
            }
            // The same order the project picker uses, so one rule covers both
            // keys rather than two that drift.
            match resolve_typed(q, pane_dir, home) {
                Some(dir) => WindowTarget::Open(dir),
                None => WindowTarget::NoSuchDirectory(q.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use crate::picker::Outcome;

    const HOME: &str = "/home/me";

    #[test]
    fn picking_a_row_opens_its_real_path_not_its_label() {
        let rows = vec!["/home/me/work/api".to_string()];
        assert_eq!(
            window_target(&Outcome::Chosen(0), &rows, "~/w/api", "/tmp", HOME),
            WindowTarget::Open("/home/me/work/api".to_string())
        );
    }

    #[test]
    fn enter_on_the_untouched_query_is_another_window_here() {
        // The whole point of prefilling: prefix and enter, nothing typed.
        assert_eq!(
            window_target(
                &Outcome::Typed("~/w/api".into()),
                &[],
                "~/w/api",
                "/home/me/work/api",
                HOME
            ),
            WindowTarget::Open("/home/me/work/api".to_string())
        );
    }

    #[test]
    fn the_pane_directory_wins_over_expanding_the_shortened_form() {
        // `~/l/yogesh.lonkar.org` cannot be expanded back: the `l` stands for
        // a directory whose name is gone. Resolving the prefill as a path
        // would either miss or, worse, find something else.
        let target = window_target(
            &Outcome::Typed("~/l/thing".into()),
            &[],
            "~/l/thing",
            "/home/me/lonkar-org/thing",
            HOME,
        );
        assert_eq!(
            target,
            WindowTarget::Open("/home/me/lonkar-org/thing".to_string())
        );
    }

    #[test]
    fn a_row_index_that_is_not_there_does_nothing() {
        assert_eq!(
            window_target(&Outcome::Chosen(7), &[], "~", "/tmp", HOME),
            WindowTarget::Cancelled
        );
    }

    #[test]
    fn cancelling_does_nothing() {
        assert_eq!(
            window_target(&Outcome::Cancelled, &[], "~", "/tmp", HOME),
            WindowTarget::Cancelled
        );
    }

    #[test]
    fn an_empty_query_does_nothing_rather_than_opening_home() {
        assert_eq!(
            window_target(&Outcome::Typed("   ".into()), &[], "~/x", "/tmp", HOME),
            WindowTarget::Cancelled
        );
    }

    #[test]
    fn a_typed_path_that_is_not_a_directory_is_reported_rather_than_guessed() {
        assert_eq!(
            window_target(
                &Outcome::Typed("nowhere-at-all".into()),
                &[],
                "~/x",
                "/tmp",
                HOME
            ),
            WindowTarget::NoSuchDirectory("nowhere-at-all".to_string())
        );
    }

    #[test]
    fn a_directory_zoxide_has_never_seen_opens_when_it_is_typed_in_full() {
        // The list is visits, so a repository cloned five minutes ago is not
        // in it. This is the only way in.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().display().to_string();
        assert_eq!(
            window_target(&Outcome::Typed(path.clone()), &[], "~/x", "/tmp", HOME),
            WindowTarget::Open(std::fs::canonicalize(&path).unwrap().display().to_string())
        );
    }

    #[test]
    fn a_relative_path_resolves_against_the_pane_first() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("child");
        std::fs::create_dir(&sub).unwrap();
        let out = window_target(
            &Outcome::Typed("child".into()),
            &[],
            "~/x",
            &dir.path().display().to_string(),
            HOME,
        );
        assert_eq!(
            out,
            WindowTarget::Open(std::fs::canonicalize(&sub).unwrap().display().to_string())
        );
    }
}

// ── what the picker shows beside a row ───────────────────────────────────────

/// Trim trailing blank lines and keep the last `max`.
///
/// A captured pane ends in whatever blank rows the prompt left behind, so the
/// interesting output would otherwise sit at the top of the preview with empty
/// space under it. Dropping the blanks first puts the newest line at the
/// bottom, which is where somebody looks.
pub fn tail_of_screen(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let last = lines.iter().rposition(|l| !l.trim().is_empty());
    let kept: &[&str] = match last {
        Some(i) => &lines[..=i],
        None => &[],
    };
    let start = kept.len().saturating_sub(max);
    kept[start..].join("\n")
}

/// A directory listing, for a project that is not open yet.
///
/// Sorted, because `read_dir` order is whatever the filesystem says and a
/// preview that reshuffles between two looks at the same directory reads as a
/// bug.
pub fn listing_of(dir: &std::path::Path, max: usize) -> String {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return String::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let shown = names.len().min(max);
    let mut out = names[..shown].join("\n");
    if names.len() > shown {
        out.push_str(&format!("\n… and {} more", names.len() - shown));
    }
    out
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn the_newest_line_ends_up_at_the_bottom() {
        // A captured pane ends in the blank rows a prompt left behind.
        let screen = "first\nsecond\nthird\n\n\n   \n";
        assert_eq!(tail_of_screen(screen, 10), "first\nsecond\nthird");
    }

    #[test]
    fn only_the_last_lines_are_kept() {
        let screen = (1..=20)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(tail_of_screen(&screen, 3), "18\n19\n20");
    }

    #[test]
    fn a_screen_of_nothing_previews_as_nothing() {
        assert_eq!(tail_of_screen("\n\n   \n", 5), "");
        assert_eq!(tail_of_screen("", 5), "");
    }

    #[test]
    fn a_listing_is_sorted_so_two_looks_agree() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["zebra", "apple", "mango"] {
            std::fs::write(dir.path().join(n), "").unwrap();
        }
        assert_eq!(listing_of(dir.path(), 10), "apple\nmango\nzebra");
    }

    #[test]
    fn a_long_listing_says_how_much_it_left_out() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            std::fs::write(dir.path().join(format!("f{i:02}")), "").unwrap();
        }
        let out = listing_of(dir.path(), 4);
        assert_eq!(out.lines().count(), 5);
        assert!(out.ends_with("… and 8 more"), "{out}");
    }

    #[test]
    fn a_directory_that_is_not_there_previews_as_nothing() {
        assert_eq!(listing_of(std::path::Path::new("/no/such/dir"), 5), "");
    }
}

/// The session attached most recently, from `list-sessions -F
/// '#{session_last_attached} #{session_name}'`.
///
/// tmux prints the timestamp as seconds since the epoch, and a session that
/// has never been attached prints ***nothing at all*** -- an empty field, not
/// a zero, so the line begins with the separator. Reading that as a parse
/// failure dropped every row on a freshly started server, which is exactly the
/// machine where `--last` is reached for. It counts as zero here, and a
/// listing where every session is unattached still answers with one of them.
///
/// A session whose name contains a space still parses, because the name is
/// everything after the first separator.
pub fn most_recent_session(listing: &str) -> Option<String> {
    listing
        .lines()
        .filter_map(|line| {
            let (stamp, name) = line.trim_end().split_once(' ')?;
            let stamp: u64 = if stamp.is_empty() {
                0
            } else {
                stamp.parse().ok()?
            };
            (!name.is_empty()).then_some((stamp, name.to_string()))
        })
        .max_by_key(|(stamp, _)| *stamp)
        .map(|(_, name)| name)
}

#[cfg(test)]
mod most_recent_tests {
    use super::*;

    #[test]
    fn the_newest_timestamp_wins() {
        let listing = "1700000000 api\n1700000900 web\n1700000500 docs\n";
        assert_eq!(most_recent_session(listing).as_deref(), Some("web"));
    }

    #[test]
    fn a_session_never_attached_does_not_win_on_its_own_merits() {
        let listing = "0 never\n1700000001 once\n";
        assert_eq!(most_recent_session(listing).as_deref(), Some("once"));
    }

    #[test]
    fn a_name_with_a_space_survives() {
        let listing = "1700000000 my project\n";
        assert_eq!(most_recent_session(listing).as_deref(), Some("my project"));
    }

    #[test]
    fn a_server_where_nothing_has_been_attached_still_answers() {
        // tmux prints an empty field, not a zero, for a session nobody has
        // attached to. Treating that as unparseable dropped every row on a
        // freshly started server.
        let listing = " first\n second\n third\n";
        assert!(most_recent_session(listing).is_some(), "{listing:?}");
    }

    #[test]
    fn an_attached_session_still_beats_an_unattached_one() {
        let listing = " never\n1700000000 once\n";
        assert_eq!(most_recent_session(listing).as_deref(), Some("once"));
    }

    #[test]
    fn nothing_listed_is_nothing_to_attach_to() {
        assert_eq!(most_recent_session(""), None);
        assert_eq!(most_recent_session("garbage\n"), None);
    }
}

/// Whether a session is one tmux named itself and nobody has done anything in.
///
/// The input is one `display-message -p` answer, tab-separated: the session
/// name, how many windows it has, how many panes the current window has, and
/// what is running in the current pane.
///
/// All four have to agree, because the point is to catch exactly one case --
/// somebody typed `tmux`, got a session called `0`, and is looking at a bare
/// shell -- and to leave every other attach alone. A session with a name was
/// asked for by name. A second window, a split, or a program running means
/// something is already happening in it.
pub fn is_an_untouched_default_session(answer: &str) -> bool {
    let mut fields = answer.trim_end().split('\t');
    let (Some(name), Some(windows), Some(panes), Some(command)) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return false;
    };
    let named_by_tmux = !name.is_empty() && name.chars().all(|c| c.is_ascii_digit());
    let untouched = windows == "1" && panes == "1";
    // A shell, under whatever name it was started with. Anything else is work.
    let shell = matches!(
        command.trim_start_matches('-'),
        "zsh" | "bash" | "sh" | "fish" | "dash" | "ksh"
    );
    named_by_tmux && untouched && shell
}

#[cfg(test)]
mod default_session_tests {
    use super::*;

    #[test]
    fn a_session_tmux_named_itself_with_one_bare_shell_qualifies() {
        assert!(is_an_untouched_default_session("0\t1\t1\tzsh"));
        assert!(is_an_untouched_default_session("12\t1\t1\tbash"));
        // A login shell prints with a leading dash.
        assert!(is_an_untouched_default_session("0\t1\t1\t-zsh"));
    }

    #[test]
    fn a_session_somebody_named_is_left_alone() {
        assert!(!is_an_untouched_default_session("api\t1\t1\tzsh"));
        assert!(!is_an_untouched_default_session("w2\t1\t1\tzsh"));
    }

    #[test]
    fn a_session_with_work_in_it_is_left_alone() {
        assert!(
            !is_an_untouched_default_session("0\t2\t1\tzsh"),
            "two windows"
        );
        assert!(!is_an_untouched_default_session("0\t1\t2\tzsh"), "a split");
        assert!(
            !is_an_untouched_default_session("0\t1\t1\tnvim"),
            "an editor"
        );
    }

    #[test]
    fn an_answer_that_is_not_four_fields_decides_nothing() {
        assert!(!is_an_untouched_default_session(""));
        assert!(!is_an_untouched_default_session("0\t1\t1"));
        assert!(!is_an_untouched_default_session("\t1\t1\tzsh"));
    }
}
