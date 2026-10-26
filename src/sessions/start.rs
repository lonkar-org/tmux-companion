//! `sessions start` and `sessions continue`: getting into a session by its name, or back into the one you left
//!
//! `start NAME@HOST`, where HOST is one of `hosts`, opens the mirror
//! session of NAME on that host, making NAME there first when it has none.
//!
//! `start NAME` looks for the name in this order and takes the first that
//! knows it:
//!
//! 1. a live session called NAME, which it attaches to (switches to, inside
//!    tmux);
//! 2. the newest snapshot generation holding a session called NAME, which it
//!    restores alone with `sessions resurrect --only NAME --merge`, confirm
//!    screen and restore table included;
//! 3. a project directory whose session would be called NAME: the directories
//!    with a saved layout and the ones `[project] dirs_source` lists. One is
//!    opened the way `project` opens it, layout and all; several open the
//!    project picker on just those;
//! 4. DIR, when given, which gets a session called NAME;
//! 5. otherwise the project picker on the directories, where a typed path is
//!    taken too. Off a terminal there is nobody to ask, so it exits 2 naming
//!    the missing argument.
//!
//! A live session wins over a snapshot because it holds more than any
//! snapshot of it, and a snapshot wins over a project because it holds the
//! windows as they were left rather than as a layout says they start.
//!
//! `continue` goes to the session used most recently that no client is in.
//! tmux cannot say which session a client detached from: in a
//! `client-detached` hook `#{client_session}` is already empty and
//! `#{session_name}` is whichever session tmux would pick for a client with
//! none, which on 3.4, 3.5a and 3.7c alike was the newest session rather than
//! the one left. So the record is the one kept on the way in instead: the
//! switcher's `@tc_ses_used` stamp, which the hooks in [`crate::switcher`] set
//! on every attach and switch without starting a process, and which this sets
//! too on its way, so the history starts with the first `continue` on a server
//! whose config does not set them. The session you were in when you detached
//! is the one stamped last that nobody is in now. Without a stamp it is
//! `session_last_attached`, whole seconds; with neither, the oldest session by
//! `session_created`. `--last` skips the records and takes the youngest
//! session by `session_created`, attached or not, other than this one.

use crate::cli::{ResurrectOptions, config_or_default, tmux_capture};

/// What `start` does with a name, decided before anything runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// A session by that name is running.
    Attach,
    /// The generation, by stamp, that holds a session by that name.
    Restore(String),
    /// The one project directory whose session has that name.
    Project(String),
    /// Several project directories whose sessions would have that name.
    Choose(Vec<String>),
    /// The directory given on the command line.
    Directory(String),
    /// Nothing knows the name: ask for a directory.
    Ask,
}

/// The order of [`Plan`], over what has been found. `projects` and `dir` are
/// only read when nothing before them answered, which is why the caller may
/// leave them empty once a live session or a snapshot has.
pub fn decide(
    live: bool,
    snapshot: Option<String>,
    projects: &[String],
    dir: Option<String>,
) -> Plan {
    if live {
        return Plan::Attach;
    }
    if let Some(stamp) = snapshot {
        return Plan::Restore(stamp);
    }
    match projects {
        [one] => return Plan::Project(one.clone()),
        [] => {}
        several => return Plan::Choose(several.to_vec()),
    }
    match dir {
        Some(d) => Plan::Directory(d),
        None => Plan::Ask,
    }
}

/// A name as tmux will keep it: it turns `.` and `:` into `_` itself, so a
/// name matched without the same change would never be found again.
pub fn tmux_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| if c == '.' || c == ':' { '_' } else { c })
        .collect()
}

/// The project directories whose session would be called `name`, in the order
/// given, each once.
pub fn matching_projects(name: &str, dirs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in dirs {
        let d = d.trim_end_matches('/');
        if d.is_empty() || crate::project::session_name(d) != name {
            continue;
        }
        if !out.iter().any(|o| o == d) {
            out.push(d.to_string());
        }
    }
    out
}

/// Whether a snapshot holds a session by this name.
pub fn holds(snap: &crate::sessions::Snapshot, name: &str) -> bool {
    snap.session.iter().any(|s| s.name == name)
}

/// The stamps to search for a name, newest first: the pointer's, then every
/// generation on disk, each once.
pub fn search_order(pointer: Option<String>, generations: &[String]) -> Vec<String> {
    let mut out: Vec<String> = pointer.into_iter().collect();
    for g in generations {
        if !out.contains(g) {
            out.push(g.clone());
        }
    }
    out
}

/// The newest generation holding a session called `name`.
fn newest_holding(state_dir: &std::path::Path, name: &str) -> Option<String> {
    use crate::sessions::store;
    let generations: Vec<String> = store::generations_in(state_dir)
        .into_iter()
        .map(|g| g.stamp)
        .collect();
    search_order(store::last_stamp_in(state_dir), &generations)
        .into_iter()
        .find(|stamp| store::load_in(state_dir, stamp).is_ok_and(|s| holds(&s, name)))
}

/// `sessions start`, answering with the exit code: `0` there, `1` for a
/// failure or a cancelled pick, `2` when the name needs a directory and there
/// is nobody to ask, or the directory given is not one.
pub async fn run(name: &str, dir: Option<String>, detach: bool) -> i32 {
    match start(name, dir, detach).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("tmux-companion: {e}");
            1
        }
    }
}

async fn start(asked: &str, dir: Option<String>, detach: bool) -> anyhow::Result<i32> {
    // `NAME@HOST` with a host after the `@`: a mirror session of NAME on
    // that host, made there when the host has no session by that name. The
    // link's mode doesn't matter: `off` stops the list, not the attach. DIR
    // is where a session made there starts, on the host, `~` its home.
    // The host is matched ignoring case when only one fits. One that matches
    // no host is refused rather than read as a local name: `api@dev2`
    // for a host called `Dev2` made a local session that looked like the
    // remote one. Only a live session here by that very name still attaches.
    if let Some((remote, typed_host)) = asked.trim().rsplit_once('@')
        && !remote.is_empty()
    {
        let hosts = crate::remote::host_names().await;
        if let Some(host) = crate::remote::match_host(typed_host, &hosts) {
            crate::remote::open_new(host, remote, true, dir.as_deref(), detach).await?;
            return Ok(0);
        }
        if !crate::cli::session_exists(&format!("={}", tmux_name(asked))).await {
            eprintln!("{}", no_such_host(typed_host, &hosts));
            return Ok(1);
        }
    }
    let name = tmux_name(asked);
    if name.is_empty() {
        eprintln!("a session needs a name");
        return Ok(2);
    }
    if name != asked {
        eprintln!("tmux keeps {asked:?} as {name:?}");
    }
    let config = config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let state_dir = crate::server::state_dir();

    let live = crate::cli::session_exists(&format!("={name}")).await;
    let snapshot = if live {
        None
    } else {
        state_dir.as_deref().and_then(|d| newest_holding(d, &name))
    };

    // The directory source is a process of its own, so it is asked only when
    // neither a live session nor a snapshot has answered.
    let mut listed: Vec<crate::project::Row> = Vec::new();
    let projects = if live || snapshot.is_some() {
        Vec::new()
    } else {
        listed = crate::project::directories(&config, &home).await;
        let mut dirs: Vec<String> = state_dir
            .as_deref()
            .map(crate::saved::projects_in)
            .unwrap_or_default();
        dirs.extend(listed.iter().map(|r| r.path.clone()));
        let dirs: Vec<String> = dirs
            .into_iter()
            .filter(|d| std::path::Path::new(d).is_dir())
            .collect();
        matching_projects(&name, &dirs)
    };

    let plan = decide(live, snapshot, &projects, dir.clone());
    if let Some(d) = &dir
        && !matches!(plan, Plan::Directory(_))
    {
        eprintln!("{d} not used: {}", said(&plan, &name));
    }

    let path = match plan {
        Plan::Attach => {
            if detach {
                println!("{name} is running");
                return Ok(0);
            }
            crate::cli::focus_session(&name).await?;
            return Ok(0);
        }
        Plan::Restore(stamp) => {
            let code = crate::sessions::cli::resurrect(ResurrectOptions {
                stamp: Some(stamp),
                only: vec![name.clone()],
                exclude: Vec::new(),
                merge: true,
                dry_run: false,
                yes: false,
                detach: true,
                hand_back: true,
            })
            .await?;
            if code != 0 {
                return Ok(code);
            }
            // A cancelled confirm screen restores nothing and still answers 0.
            if !crate::cli::session_exists(&format!("={name}")).await {
                return Ok(1);
            }
            if !detach {
                crate::cli::focus_session(&name).await?;
            }
            return Ok(0);
        }
        Plan::Project(path) => path,
        Plan::Directory(d) => {
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            match crate::project::resolve_typed(&d, &cwd, &home) {
                Some(path) => path,
                None => {
                    eprintln!("no such directory: {d}");
                    return Ok(2);
                }
            }
        }
        Plan::Choose(paths) => {
            let rows: Vec<crate::project::Row> =
                paths.iter().map(|p| directory_row(p, &listed)).collect();
            if !watched() {
                eprintln!(
                    "{} projects would be called {name}; give the one you mean:",
                    paths.len()
                );
                for p in &paths {
                    eprintln!("  tmux-companion sessions start {name} {p}");
                }
                return Ok(2);
            }
            match pick(&name, &rows, &config, &home)? {
                Some(path) => path,
                None => return Ok(1),
            }
        }
        Plan::Ask => {
            // An empty picker closes before anything can be typed, so with no
            // directory source the argument is the only way to say where.
            if !watched() || listed.is_empty() {
                eprintln!(
                    "no session, snapshot or project is called {name}, and the directory to \
                     start it in is missing: tmux-companion sessions start {name} DIR"
                );
                return Ok(2);
            }
            match pick(&name, &listed, &config, &home)? {
                Some(path) => path,
                None => return Ok(1),
            }
        }
    };

    crate::cli::build_project_session(&name, &path, &config, &home).await;
    if !crate::cli::session_exists(&format!("={name}")).await {
        eprintln!("tmux did not start {name} in {path}");
        return Ok(1);
    }
    if detach {
        println!(
            "started {name} in {}",
            crate::project::short_path(&path, &home)
        );
        return Ok(0);
    }
    crate::cli::focus_session(&name).await?;
    Ok(0)
}

/// The refusal for `NAME@HOST` whose HOST is none of `hosts`, naming them.
fn no_such_host(typed: &str, hosts: &[String]) -> String {
    if hosts.is_empty() {
        format!("no host called {typed}, and none is configured: tmux-companion hosts add")
    } else {
        format!("no host called {typed}; the hosts are {}", hosts.join(", "))
    }
}

/// Why a DIR on the command line went unused, in words.
fn said(plan: &Plan, name: &str) -> String {
    match plan {
        Plan::Attach => format!("{name} is running"),
        Plan::Restore(stamp) => format!("{name} is in the snapshot from {stamp}"),
        Plan::Project(path) => format!("{name} is the project at {path}"),
        Plan::Choose(_) => format!("several projects are called {name}"),
        Plan::Directory(_) | Plan::Ask => String::new(),
    }
}

/// Whether there is somebody at a terminal to ask.
fn watched() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// A picker row for a directory, the source's own when it listed it, so the
/// theme colour comes along.
fn directory_row(path: &str, listed: &[crate::project::Row]) -> crate::project::Row {
    listed
        .iter()
        .find(|r| r.path == path)
        .cloned()
        .unwrap_or_else(|| crate::project::Row {
            kind: crate::project::Kind::Directory,
            label: crate::project::session_name(path),
            path: path.to_string(),
            colour: None,
            idle: None,
            mirror: None,
        })
}

/// The project picker over `rows`, with a typed path taken as well: the
/// directory to start `name` in, or `None` when nothing was chosen.
fn pick(
    name: &str,
    rows: &[crate::project::Row],
    config: &crate::config::Config,
    home: &str,
) -> anyhow::Result<Option<String>> {
    use crate::cli::ProjectPick;
    let chrome = crate::picker::Chrome {
        title: format!("[ Where does {name} start ]"),
        icon: crate::tmux::icons::SESSION.into(),
        footer: "enter start here   esc cancel   type a path for another".into(),
        preview_title: "[ Where ]".into(),
        ..Default::default()
    }
    .laid_out_like(&config.picker, crate::config::Picker::Project);
    let items = crate::cli::project_items(rows, config, home);
    let outcome = crate::picker::run_with_query(items, "", &chrome)?;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    Ok(
        match crate::cli::project_pick(&outcome, rows, &cwd, home, None) {
            ProjectPick::Directory(path) => Some(path),
            ProjectPick::Missing(typed) => {
                eprintln!("no such directory: {typed}");
                None
            }
            ProjectPick::Session(_) | ProjectPick::HostDirectory(..) | ProjectPick::Nothing => {
                eprintln!("nothing started");
                None
            }
        },
    )
}

// ── continue ────────────────────────────────────────────────────────────────

/// What `continue` asks `list-sessions` for, tab-separated, the name last
/// because it may hold anything.
pub const CONTINUE_FORMAT: &str = "#{session_id}\t#{session_attached}\t#{@tc_ses_used}\t#{session_last_attached}\t#{session_created}\t#{session_name}";

/// One live session, as [`CONTINUE_FORMAT`] lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The number in `$12`, which tmux hands out in creation order and so
    /// breaks a tie in `created`.
    pub id: u64,
    /// Clients attached to it now.
    pub attached: u32,
    /// The switcher's stamp from the last switch to it.
    pub used: Option<u64>,
    /// When a client last attached to it, in seconds; tmux prints nothing
    /// for one never attached.
    pub last_attached: Option<u64>,
    /// When it was created, in seconds.
    pub created: u64,
    /// `#{session_name}`.
    pub name: String,
}

/// Parse a [`CONTINUE_FORMAT`] listing, dropping a line that does not read.
pub fn parse_listing(listing: &str) -> Vec<Listed> {
    listing
        .lines()
        .filter_map(|line| {
            let mut f = line.splitn(6, '\t');
            let id = f.next()?.trim().trim_start_matches('$').parse().ok()?;
            let attached = f.next()?.trim().parse().ok()?;
            let used = f.next()?.trim().parse().ok();
            let last_attached = f.next()?.trim().parse().ok().filter(|t| *t > 0);
            let created = f.next()?.trim().parse().ok()?;
            let name = f.next()?.to_string();
            (!name.is_empty()).then_some(Listed {
                id,
                attached,
                used,
                last_attached,
                created,
                name,
            })
        })
        .collect()
}

/// Which record chose the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The switcher's stamp.
    Used,
    /// `session_last_attached`.
    Attached,
    /// No record: the oldest session.
    Oldest,
    /// `--last`: the youngest, whatever the records say.
    Youngest,
}

impl Why {
    /// The reason in words, as `--print` gives it.
    pub fn words(self) -> &'static str {
        match self {
            Why::Used => "used most recently",
            Why::Attached => "attached most recently",
            Why::Oldest => "oldest, nothing recorded which was used last",
            Why::Youngest => "youngest",
        }
    }
}

/// The session to continue, and which record said so.
///
/// Never `current`, the session this runs in. The sessions no client is in
/// come first, since the one you detached from is one of them; when every
/// other session has a client, those are the choice instead. Among them the
/// highest switcher stamp, then the latest attach time, then the oldest by
/// creation. With `youngest` the records are not read: the session created
/// last among every other one, attached or not.
pub fn pick_continue(
    sessions: &[Listed],
    current: Option<&str>,
    youngest: bool,
) -> Option<(String, Why)> {
    let others: Vec<&Listed> = sessions
        .iter()
        .filter(|s| Some(s.name.as_str()) != current)
        .collect();
    let by_age = |s: &&&Listed| (s.created, s.id);
    if youngest {
        return others
            .iter()
            .max_by_key(by_age)
            .map(|s| (s.name.clone(), Why::Youngest));
    }
    let free: Vec<&Listed> = others.iter().copied().filter(|s| s.attached == 0).collect();
    let pool = if free.is_empty() { others } else { free };

    if let Some(s) = pool
        .iter()
        .filter(|s| s.used.is_some())
        .max_by_key(|s| (s.used, s.id))
    {
        return Some((s.name.clone(), Why::Used));
    }
    if let Some(s) = pool
        .iter()
        .filter(|s| s.last_attached.is_some())
        .max_by_key(|s| (s.last_attached, s.id))
    {
        return Some((s.name.clone(), Why::Attached));
    }
    pool.iter()
        .min_by_key(by_age)
        .map(|s| (s.name.clone(), Why::Oldest))
}

/// `sessions continue`, answering with the exit code: `0` there (or printed),
/// `1` when there is no other session to go to.
pub async fn run_continue(last: bool, print: bool) -> i32 {
    match continue_(last, print).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("tmux-companion: {e}");
            1
        }
    }
}

async fn continue_(last: bool, print: bool) -> anyhow::Result<i32> {
    // The session this runs in, which is never the one to continue. By the
    // pane rather than the client: from a popup or `run-shell` the client
    // tmux would pick is whichever was busy last.
    let current = match std::env::var("TMUX_PANE") {
        Ok(pane) if std::env::var_os("TMUX").is_some() => {
            let s = tmux_capture(&["display-message", "-p", "-t", &pane, "#{session_name}"]).await;
            Some(s.trim().to_string()).filter(|s| !s.is_empty())
        }
        _ => None,
    };

    // The switcher's hooks set on the way, in the same tmux call as the
    // listing: they only set options, and with them set the next detach has a
    // record to come back to.
    let mut args: Vec<String> = Vec::new();
    for cmd in crate::switcher::hooks() {
        args.extend(cmd);
        args.push(";".to_string());
    }
    args.extend(
        [
            "list-sessions",
            "-f",
            crate::link::NOT_MIRRORED,
            "-F",
            CONTINUE_FORMAT,
        ]
        .map(String::from),
    );
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let listing = tmux_capture(&borrowed).await;
    let sessions = parse_listing(&listing);

    if sessions.is_empty() {
        eprintln!(
            "no tmux server running, nothing to continue. \
             `tmux-companion sessions resurrect` brings back the last snapshot"
        );
        return Ok(1);
    }
    let Some((name, why)) = pick_continue(&sessions, current.as_deref(), last) else {
        eprintln!("no other session to continue");
        return Ok(1);
    };
    if print {
        println!("{name}\t{}", why.words());
        return Ok(0);
    }
    crate::cli::focus_session(&name).await?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(
        id: u64,
        attached: u32,
        used: Option<u64>,
        last: Option<u64>,
        created: u64,
        name: &str,
    ) -> Listed {
        Listed {
            id,
            attached,
            used,
            last_attached: last,
            created,
            name: name.to_string(),
        }
    }

    #[test]
    fn a_live_session_wins_over_everything_else() {
        let p = decide(
            true,
            Some("s".into()),
            &["/w/api".into()],
            Some("/d".into()),
        );
        assert_eq!(p, Plan::Attach);
    }

    #[test]
    fn a_snapshot_wins_over_a_project_and_the_directory() {
        let p = decide(
            false,
            Some("s".into()),
            &["/w/api".into()],
            Some("/d".into()),
        );
        assert_eq!(p, Plan::Restore("s".into()));
    }

    #[test]
    fn one_project_opens_and_several_are_a_choice() {
        assert_eq!(
            decide(false, None, &["/w/api".into()], Some("/d".into())),
            Plan::Project("/w/api".into())
        );
        assert_eq!(
            decide(false, None, &["/a/api".into(), "/b/api".into()], None),
            Plan::Choose(vec!["/a/api".into(), "/b/api".into()])
        );
    }

    #[test]
    fn the_directory_given_comes_last_and_nothing_at_all_asks() {
        assert_eq!(
            decide(false, None, &[], Some("/d".into())),
            Plan::Directory("/d".into())
        );
        assert_eq!(decide(false, None, &[], None), Plan::Ask);
    }

    #[test]
    fn a_name_is_matched_as_tmux_will_keep_it() {
        assert_eq!(tmux_name(" my.app:2 "), "my_app_2");
        assert_eq!(tmux_name("api"), "api");
    }

    #[test]
    fn projects_match_on_the_session_name_their_directory_gives() {
        let dirs = vec![
            "/w/api".to_string(),
            "/w/web".to_string(),
            "/x/api/".to_string(),
            "/w/api".to_string(),
            "/w/my.app".to_string(),
        ];
        assert_eq!(matching_projects("api", &dirs), vec!["/w/api", "/x/api"]);
        assert_eq!(matching_projects("my_app", &dirs), vec!["/w/my.app"]);
        assert!(matching_projects("nope", &dirs).is_empty());
    }

    #[test]
    fn the_pointer_is_searched_first_and_nothing_twice() {
        assert_eq!(
            search_order(Some("b".into()), &["c".into(), "b".into(), "a".into()]),
            vec!["b", "c", "a"]
        );
        assert_eq!(search_order(None, &["c".into()]), vec!["c"]);
    }

    #[test]
    fn a_snapshot_holds_a_session_by_its_exact_name() {
        let snap = crate::sessions::Snapshot {
            session: vec![crate::sessions::Session {
                name: "api".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(holds(&snap, "api"));
        assert!(!holds(&snap, "ap"));
    }

    #[test]
    fn the_listing_reads_with_its_empty_fields() {
        let listing =
            "$1\t0\t\t\t1700000000\talpha\n$2\t1\t7\t1700000100\t1700000050\tbe\tta\nnonsense\n";
        assert_eq!(
            parse_listing(listing),
            vec![
                s(1, 0, None, None, 1_700_000_000, "alpha"),
                s(2, 1, Some(7), Some(1_700_000_100), 1_700_000_050, "be\tta"),
            ]
        );
    }

    #[test]
    fn the_stamp_decides_among_the_sessions_nobody_is_in() {
        let l = vec![
            s(1, 0, Some(3), Some(50), 10, "old"),
            s(2, 0, Some(9), Some(40), 20, "left"),
            s(3, 1, Some(12), Some(60), 30, "in-use"),
        ];
        assert_eq!(
            pick_continue(&l, None, false),
            Some(("left".into(), Why::Used))
        );
    }

    #[test]
    fn the_session_this_runs_in_is_never_the_answer() {
        let l = vec![
            s(1, 1, Some(3), None, 10, "here"),
            s(2, 0, Some(9), None, 20, "there"),
        ];
        assert_eq!(
            pick_continue(&l, Some("there"), false),
            Some(("here".into(), Why::Used))
        );
        assert_eq!(pick_continue(&l[..1], Some("here"), false), None);
    }

    #[test]
    fn with_every_other_session_attached_one_of_those_is_the_answer() {
        let l = vec![
            s(1, 1, Some(3), None, 10, "a"),
            s(2, 1, Some(5), None, 20, "b"),
        ];
        assert_eq!(
            pick_continue(&l, None, false),
            Some(("b".into(), Why::Used))
        );
    }

    #[test]
    fn without_a_stamp_the_attach_time_decides() {
        let l = vec![
            s(1, 0, None, Some(100), 10, "a"),
            s(2, 0, None, Some(200), 20, "b"),
            s(3, 0, None, None, 30, "c"),
        ];
        assert_eq!(
            pick_continue(&l, None, false),
            Some(("b".into(), Why::Attached))
        );
    }

    #[test]
    fn last_takes_the_youngest_whatever_the_records_say() {
        let l = vec![
            s(1, 0, Some(9), Some(200), 10, "stamped"),
            s(2, 1, None, None, 40, "young-attached"),
            s(3, 0, None, None, 50, "here"),
        ];
        assert_eq!(
            pick_continue(&l, Some("here"), true),
            Some(("young-attached".into(), Why::Youngest))
        );
        assert_eq!(pick_continue(&l[2..], Some("here"), true), None);
    }

    #[test]
    fn with_no_record_the_oldest_and_with_last_the_youngest() {
        let l = vec![
            s(2, 0, None, None, 10, "second"),
            s(1, 0, None, None, 10, "first"),
            s(3, 0, None, None, 30, "third"),
        ];
        assert_eq!(
            pick_continue(&l, None, false),
            Some(("first".into(), Why::Oldest))
        );
        assert_eq!(
            pick_continue(&l, None, true),
            Some(("third".into(), Why::Youngest))
        );
    }
}
