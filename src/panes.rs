//! Every pane on the server, as a list to jump from, and the agents among
//! them.
//!
//! The question this answers is "where is everything", asked by somebody
//! running four agents across three projects who has lost track of which one
//! asked a question ten minutes ago. tmux's own `choose-tree` shows the same
//! panes, but it shows them as a tree of names, and the name of a pane is the
//! hostname unless something set it. What tells the two `claude` panes apart is
//! the directory, whether one has gone quiet, and the last few lines on its
//! screen, so those are the columns and that is the preview.
//!
//! Quiet is measured on the window rather than the pane, because tmux has no
//! per-pane activity time. `#{window_activity}` is the last time anything in
//! that window wrote to its screen, which for a window holding one agent is
//! the agent, and for a window holding an agent beside a shell you are typing
//! in is you. That is a known limit rather than a bug, and the manual says so.
//!
//! The one `list-panes -a` here is shared with the bar's `agents` segment,
//! which counts the same rows the picker shows, so the two cannot disagree
//! about what an agent is or when one is waiting.
//!
//! This runs in the **client**, and talks to tmux directly: there is nothing
//! for a daemon to cache in a list somebody is about to pick from, and the
//! picker needs the terminal the daemon does not have.

use std::fmt;

/// The colour of a waiting agent's row, and of the waiting count on the bar.
///
/// An orange between the battery's yellow and its red: something that wants a
/// look, not something that has gone wrong. Fixed like the other segments'
/// colours rather than taken from the theme, which paints sessions, not the
/// bar.
pub const WAITING_COLOUR: &str = "colour214";

/// One pane as tmux reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    /// `#{session_name}`.
    pub session: String,
    /// `#{window_index}`.
    pub window_index: u32,
    /// `#{window_name}`.
    pub window_name: String,
    /// `#{pane_index}`.
    pub pane_index: u32,
    /// `#{pane_id}`, the `%N` that stays the same when panes move.
    pub id: String,
    /// `#{pane_current_command}`.
    pub command: String,
    /// `#{pane_current_path}`.
    pub path: String,
    /// `#{pane_title}`, which is the hostname until something sets it.
    pub title: String,
    /// Whether somebody could see this pane at the moment of the scan.
    pub visible: bool,
    /// `#{window_activity}`: when the window last drew, in unix seconds.
    pub activity: u64,
    /// `#{pane_in_mode}`: copy mode, so somebody is reading it.
    pub in_mode: bool,
    /// `#{host}`, which is what a pane's title is until a program sets one.
    pub host: String,
}

/// The `-F` string the listing asks for.
///
/// `#{host}` rides along so the title can be compared against tmux's own
/// default for it without a second call: a pane whose title is the hostname
/// has no title worth showing.
pub fn pane_format() -> &'static str {
    "#{session_name}\t#{window_index}\t#{window_name}\t#{pane_index}\t#{pane_id}\t\
     #{pane_current_command}\t#{pane_current_path}\t#{pane_title}\t#{pane_active}\t\
     #{window_active}\t#{session_attached}\t#{window_activity}\t#{pane_in_mode}\t#{host}"
}

/// How many fields [`pane_format`] produces.
const FIELDS: usize = 14;

/// Parse what [`pane_format`] produces, sorted by session, window and pane.
///
/// A line with the wrong number of fields is skipped rather than guessed at:
/// a window name can hold a tab if somebody is determined, and a row built
/// from shifted fields would point the jump at the wrong pane.
pub fn parse(text: &str) -> Vec<Pane> {
    let mut panes: Vec<Pane> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() != FIELDS {
                return None;
            }
            let on = |s: &str| s.trim() == "1";
            let attached: u32 = f[10].trim().parse().unwrap_or(0);
            Some(Pane {
                session: f[0].to_string(),
                window_index: f[1].trim().parse().ok()?,
                window_name: f[2].to_string(),
                pane_index: f[3].trim().parse().ok()?,
                id: f[4].trim().to_string(),
                command: f[5].trim().to_string(),
                path: f[6].to_string(),
                title: f[7].to_string(),
                visible: on(f[8]) && on(f[9]) && attached > 0,
                activity: f[11].trim().parse().unwrap_or(0),
                in_mode: on(f[12]),
                host: f[13].trim().to_string(),
            })
        })
        .collect();
    panes.sort_by(|a, b| {
        a.session
            .cmp(&b.session)
            .then(a.window_index.cmp(&b.window_index))
            .then(a.pane_index.cmp(&b.pane_index))
    });
    panes
}

/// Whether a command is one of the configured agents.
pub fn is_agent(command: &str, programs: &[String]) -> bool {
    programs.iter().any(|p| p == command)
}

/// What a pane is doing, as far as a window's activity time can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// In copy mode: somebody is looking at it, whatever it is doing.
    Reading,
    /// An agent that drew something recently.
    Busy,
    /// An agent that has been quiet this long, in seconds.
    Waiting(u64),
    /// Anything else that drew something recently.
    Active,
    /// Anything else that has been quiet this long, in seconds.
    Idle(u64),
}

impl State {
    /// An agent that has stopped and may be waiting on a person.
    pub fn is_waiting(self) -> bool {
        matches!(self, State::Waiting(_))
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            State::Reading => f.write_str("reading"),
            State::Busy => f.write_str("busy"),
            State::Waiting(secs) => write!(f, "waiting {}", age(*secs)),
            State::Active => f.write_str("active"),
            State::Idle(secs) => write!(f, "idle {}", age(*secs)),
        }
    }
}

/// Classify one pane at a given moment.
///
/// Copy mode wins over everything: a pane somebody is reading is not one that
/// needs pointing out. Then the window's quiet time against `waiting_secs`,
/// with agents and everything else getting different words for the same two
/// answers, because "idle" is what a shell is most of the day and "waiting" is
/// what an agent is when it has asked you something.
pub fn state(pane: &Pane, agent: bool, now: u64, waiting_secs: u64) -> State {
    if pane.in_mode {
        return State::Reading;
    }
    let quiet = now.saturating_sub(pane.activity);
    match (agent, quiet < waiting_secs) {
        (true, true) => State::Busy,
        (true, false) => State::Waiting(quiet),
        (false, true) => State::Active,
        (false, false) => State::Idle(quiet),
    }
}

/// Seconds as `12s`, `3m`, `2h` or `1d`: one unit, rounded down.
///
/// The column is read at a glance to answer "how long ago", and `3m` answers
/// that where `3m 07s` makes the eye stop.
pub fn age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// The program column: the command, and the pane's title when it says more.
///
/// A title is shown only when a program set one. tmux's default is the
/// hostname, and a shell that sets its title to its own name adds nothing to
/// the command already there.
pub fn program(pane: &Pane) -> String {
    let title = pane.title.trim();
    if title.is_empty() || title == pane.host || title == pane.command {
        return pane.command.clone();
    }
    format!("{} \u{2014} {title}", pane.command)
}

/// Which panes the list holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Filter<'a> {
    /// The pane this is running in, which is left out: jumping to where you
    /// already are is nothing.
    pub exclude: Option<&'a str>,
    /// Only this session's panes.
    pub session: Option<&'a str>,
    /// Only the panes running one of `[agents] programs`.
    pub only_agents: bool,
}

/// One row of the picker, or one line of `--print`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// `session:window.pane`.
    pub at: String,
    /// What is running, with the title when it says more.
    pub program: String,
    /// Busy, waiting, reading, or the plain equivalents.
    pub state: State,
    /// The directory, shortened the way the project picker does.
    pub cwd: String,
    /// The `%N` the jump targets.
    pub id: String,
    /// Whether the program is a configured agent.
    pub agent: bool,
}

/// Everything a row needs that is not the pane.
#[derive(Debug, Clone, Copy)]
pub struct Clock<'a> {
    /// Now, in unix seconds, supplied so a test can fix it.
    pub now: u64,
    /// `[agents] waiting_secs`.
    pub waiting_secs: u64,
    /// `[agents] programs`.
    pub programs: &'a [String],
    /// The home directory, for the short path.
    pub home: &'a str,
}

/// The rows a listing produces, in the order [`parse`] left them.
pub fn rows(panes: &[Pane], filter: &Filter, clock: &Clock) -> Vec<Row> {
    panes
        .iter()
        .filter(|p| filter.exclude != Some(p.id.as_str()))
        .filter(|p| filter.session.is_none_or(|s| s == p.session))
        .filter_map(|p| {
            let agent = is_agent(&p.command, clock.programs);
            if filter.only_agents && !agent {
                return None;
            }
            Some(Row {
                at: format!("{}:{}.{}", p.session, p.window_index, p.pane_index),
                program: program(p),
                state: state(p, agent, clock.now, clock.waiting_secs),
                cwd: crate::project::short_path(&p.path, clock.home),
                id: p.id.clone(),
                agent,
            })
        })
        .collect()
}

/// The last `max` lines of a capture that say anything, trailing space
/// stripped.
///
/// A screen is mostly the blank rows under the prompt, and a preview that
/// shows those puts the one line that matters at the top with nothing under
/// it. Blank lines in the middle go too, so fifteen lines is fifteen lines of
/// output rather than five and a gap.
pub fn preview_tail(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect();
    let start = lines.len().saturating_sub(max);
    lines[start..].join("\n")
}

/// Rows beyond this get no preview: a capture per row is a few milliseconds
/// each, and a server with more panes than this is one where the picker
/// opening at once matters more than what is on the two-hundredth screen.
pub const PREVIEW_LIMIT: usize = 200;

/// How many lines of each screen the preview keeps.
pub const PREVIEW_LINES: usize = 15;

/// How far back each capture reads. More than the preview keeps, so blank
/// lines dropped by [`preview_tail`] do not leave it short.
const CAPTURE_LINES: &str = "-40";

/// Now, in unix seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Every pane on the server. Empty when tmux is not there to ask.
pub async fn list() -> Vec<Pane> {
    parse(&crate::cli::tmux_capture(&["list-panes", "-a", "-F", pane_format()]).await)
}

/// The last lines of one pane's screen, as the preview shows them.
pub async fn tail_of(id: &str) -> String {
    let screen =
        crate::cli::tmux_capture(&["capture-pane", "-p", "-t", id, "-S", CAPTURE_LINES]).await;
    preview_tail(&screen, PREVIEW_LINES)
}

/// The last lines of each row's screen, in row order.
///
/// Captured concurrently, because two hundred sequential tmux calls at a few
/// milliseconds each is a popup that opens with a visible pause.
async fn previews(rows: &[Row]) -> Vec<String> {
    if rows.len() > PREVIEW_LIMIT {
        return vec![String::new(); rows.len()];
    }
    let mut set = tokio::task::JoinSet::new();
    for (i, row) in rows.iter().enumerate() {
        let id = row.id.clone();
        set.spawn(async move {
            let screen =
                crate::cli::tmux_capture(&["capture-pane", "-p", "-t", &id, "-S", CAPTURE_LINES])
                    .await;
            (i, preview_tail(&screen, PREVIEW_LINES))
        });
    }
    let mut out = vec![String::new(); rows.len()];
    while let Some(Ok((i, text))) = set.join_next().await {
        out[i] = text;
    }
    out
}

/// Put the attached client on a pane.
///
/// `switch-client` with a pane target resolves the session, selects the
/// window and activates the pane in one go; the two calls after it are there
/// for the case where the client was already in that session, which
/// `switch-client` treats as nothing to do. From a popup the client to move is
/// the attached one, not the popup's own, which is the same lookup the project
/// picker makes.
pub async fn jump(id: &str) {
    if std::env::var_os("TMUX").is_none() {
        crate::cli::tmux(&["attach-session", "-t", id]).await;
        return;
    }
    let client = crate::cli::attached_client().await;
    let mut args = vec!["switch-client"];
    if let Some((name, _)) = &client {
        args.extend(["-c", name.as_str()]);
    }
    args.extend(["-t", id]);
    crate::cli::tmux(&args).await;
    crate::cli::tmux(&["select-window", "-t", id]).await;
    crate::cli::tmux(&["select-pane", "-t", id]).await;
}

/// `panes`: list every pane, or every agent, and jump to the one picked.
///
/// Talks to tmux directly and never to the daemon, so there is no args struct
/// for it in `proto.rs`: the list is built and thrown away in the time it
/// takes to read it, and the picker needs this process's terminal.
pub async fn run(agents: bool, print: bool, target: Option<String>) -> anyhow::Result<()> {
    let config = crate::cli::config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let here = std::env::var("TMUX_PANE").ok();

    let panes = list().await;
    let filter = Filter {
        exclude: here.as_deref(),
        session: target.as_deref(),
        only_agents: agents,
    };
    let clock = Clock {
        now: now_secs(),
        waiting_secs: config.agents.waiting_secs,
        programs: &config.agents.programs,
        home: &home,
    };
    let rows = rows(&panes, &filter, &clock);

    if rows.is_empty() {
        eprintln!(
            "{}",
            if agents {
                "no agent panes running"
            } else {
                "no panes to show"
            }
        );
        return Ok(());
    }

    if print {
        for r in &rows {
            println!("{}\t{}\t{}\t{}\t{}", r.at, r.program, r.state, r.cwd, r.id);
        }
        return Ok(());
    }

    let previews = previews(&rows).await;
    let items: Vec<crate::picker::Item> = rows
        .iter()
        .zip(previews)
        .map(|(r, preview)| {
            let state = r.state.to_string();
            crate::picker::Item::with_preview(
                format!("{} {} {} {}", r.at, r.program, state, r.cwd),
                preview,
            )
            .in_columns(vec![r.at.clone(), r.program.clone(), state, r.cwd.clone()])
            // The same colour the bar uses for the waiting count, so the row
            // that wants you is the one that stands out here too.
            .in_colour(r.state.is_waiting().then(|| WAITING_COLOUR.to_string()))
        })
        .collect();

    let chrome = crate::picker::Chrome {
        title: if agents { "[ Agents ]" } else { "[ Panes ]" }.into(),
        footer: "enter jumps there   ctrl-a clears the filter   esc cancels".into(),
        preview_title: "[ Screen ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Panes);

    if let Some(index) = crate::picker::run(items, "", &chrome)?
        && let Some(row) = rows.get(index)
    {
        jump(&row.id).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One line as tmux would print it.
    #[allow(clippy::too_many_arguments)]
    fn line(
        session: &str,
        window: u32,
        pane: u32,
        id: &str,
        command: &str,
        path: &str,
        title: &str,
        activity: u64,
        in_mode: bool,
    ) -> String {
        format!(
            "{session}\t{window}\tedit\t{pane}\t{id}\t{command}\t{path}\t{title}\t1\t1\t1\t{activity}\t{}\tlaptop\n",
            u8::from(in_mode)
        )
    }

    fn pane(command: &str, activity: u64, in_mode: bool) -> Pane {
        parse(&line(
            "api",
            2,
            1,
            "%7",
            command,
            "/home/me/w/api",
            "laptop",
            activity,
            in_mode,
        ))
        .remove(0)
    }

    fn programs() -> Vec<String> {
        crate::config::Agents::default().programs
    }

    #[test]
    fn the_format_and_the_parser_agree_on_the_field_count() {
        assert_eq!(pane_format().split('\t').count(), FIELDS);
    }

    #[test]
    fn a_line_parses_into_every_field() {
        let p = pane("claude", 1_700_000_000, false);
        assert_eq!(p.session, "api");
        assert_eq!(p.window_index, 2);
        assert_eq!(p.window_name, "edit");
        assert_eq!(p.pane_index, 1);
        assert_eq!(p.id, "%7");
        assert_eq!(p.command, "claude");
        assert_eq!(p.path, "/home/me/w/api");
        assert_eq!(p.title, "laptop");
        assert!(p.visible);
        assert_eq!(p.activity, 1_700_000_000);
        assert!(!p.in_mode);
        assert_eq!(p.host, "laptop");
    }

    #[test]
    fn a_short_line_is_skipped_rather_than_shifting_the_fields() {
        assert!(parse("api\t2\tedit\t1\t%7\tclaude\n").is_empty());
        assert!(parse("\n\n").is_empty());
    }

    #[test]
    fn rows_come_back_by_session_then_window_then_pane() {
        let text = [
            line("web", 1, 0, "%3", "zsh", "/w", "laptop", 0, false),
            line("api", 3, 0, "%2", "zsh", "/a", "laptop", 0, false),
            line("api", 1, 1, "%9", "zsh", "/a", "laptop", 0, false),
            line("api", 1, 0, "%1", "zsh", "/a", "laptop", 0, false),
        ]
        .concat();
        let ids: Vec<String> = parse(&text).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["%1", "%9", "%2", "%3"]);
    }

    #[test]
    fn a_pane_is_visible_only_when_somebody_could_see_it() {
        let hidden = "api\t2\tedit\t1\t%7\tclaude\t/a\tlaptop\t1\t0\t1\t0\t0\tlaptop\n";
        assert!(!parse(hidden)[0].visible, "window not active");
        let detached = "api\t2\tedit\t1\t%7\tclaude\t/a\tlaptop\t1\t1\t0\t0\t0\tlaptop\n";
        assert!(!parse(detached)[0].visible, "nobody attached");
    }

    // ── state ───────────────────────────────────────────────────────────────

    #[test]
    fn an_agent_that_drew_recently_is_busy_and_a_quiet_one_is_waiting() {
        let now = 1_000;
        assert_eq!(
            state(&pane("claude", 995, false), true, now, 10),
            State::Busy
        );
        assert_eq!(
            state(&pane("claude", 800, false), true, now, 10),
            State::Waiting(200)
        );
        // Exactly the threshold is waiting: ten seconds of nothing is what
        // the setting says counts.
        assert_eq!(
            state(&pane("claude", 990, false), true, now, 10),
            State::Waiting(10)
        );
    }

    #[test]
    fn everything_else_gets_the_plain_words() {
        let now = 1_000;
        assert_eq!(
            state(&pane("zsh", 999, false), false, now, 10),
            State::Active
        );
        assert_eq!(
            state(&pane("zsh", 0, false), false, now, 10),
            State::Idle(1_000)
        );
    }

    #[test]
    fn copy_mode_is_reading_whatever_else_is_true() {
        // Somebody is looking at it, so it is not a pane that needs pointing
        // out, and it is not counted as waiting on the bar either.
        let s = state(&pane("claude", 0, true), true, 1_000, 10);
        assert_eq!(s, State::Reading);
        assert!(!s.is_waiting());
    }

    #[test]
    fn a_clock_that_runs_backwards_does_not_panic() {
        // tmux's clock and this process's are two reads of the same thing,
        // and a window that drew between them reports a future time.
        assert_eq!(
            state(&pane("zsh", 2_000, false), false, 1_000, 10),
            State::Active
        );
    }

    #[test]
    fn ages_read_as_one_unit() {
        assert_eq!(age(0), "0s");
        assert_eq!(age(12), "12s");
        assert_eq!(age(59), "59s");
        assert_eq!(age(60), "1m");
        assert_eq!(age(3 * 60 + 59), "3m");
        assert_eq!(age(2 * 3600 + 100), "2h");
        assert_eq!(age(86_400), "1d");
        assert_eq!(age(3 * 86_400 + 5), "3d");
    }

    #[test]
    fn states_are_worded_for_the_column() {
        assert_eq!(State::Reading.to_string(), "reading");
        assert_eq!(State::Busy.to_string(), "busy");
        assert_eq!(State::Waiting(180).to_string(), "waiting 3m");
        assert_eq!(State::Active.to_string(), "active");
        assert_eq!(State::Idle(7_200).to_string(), "idle 2h");
    }

    // ── the program column ──────────────────────────────────────────────────

    #[test]
    fn a_title_is_shown_only_when_a_program_set_one() {
        assert_eq!(program(&pane("claude", 0, false)), "claude", "hostname");
        let mut p = pane("zsh", 0, false);
        p.title = "zsh".into();
        assert_eq!(program(&p), "zsh", "the command again");
        p.title = String::new();
        assert_eq!(program(&p), "zsh", "empty");
        p.command = "claude".into();
        p.title = "claude: cache".into();
        assert_eq!(program(&p), "claude \u{2014} claude: cache");
    }

    // ── rows ────────────────────────────────────────────────────────────────

    fn a_server() -> Vec<Pane> {
        parse(
            &[
                line(
                    "api",
                    1,
                    0,
                    "%1",
                    "nvim",
                    "/home/me/w/api",
                    "laptop",
                    990,
                    false,
                ),
                line(
                    "api",
                    2,
                    1,
                    "%2",
                    "claude",
                    "/home/me/w/api",
                    "laptop",
                    800,
                    false,
                ),
                line(
                    "web",
                    1,
                    0,
                    "%3",
                    "zsh",
                    "/home/me/w/web",
                    "laptop",
                    999,
                    false,
                ),
                line(
                    "web",
                    1,
                    1,
                    "%4",
                    "codex",
                    "/home/me/w/web",
                    "laptop",
                    995,
                    false,
                ),
            ]
            .concat(),
        )
    }

    fn clock(programs: &[String]) -> Clock<'_> {
        Clock {
            now: 1_000,
            waiting_secs: 10,
            programs,
            home: "/home/me",
        }
    }

    #[test]
    fn a_row_carries_the_four_columns_and_the_id() {
        let programs = programs();
        let rows = rows(&a_server(), &Filter::default(), &clock(&programs));
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].at, "api:2.1");
        assert_eq!(rows[1].program, "claude");
        assert_eq!(rows[1].state, State::Waiting(200));
        assert_eq!(rows[1].cwd, "~/w/api");
        assert_eq!(rows[1].id, "%2");
        assert!(rows[1].agent);
        assert!(!rows[0].agent);
    }

    #[test]
    fn the_pane_this_runs_in_is_left_out() {
        // Jumping to where you are is nothing.
        let programs = programs();
        let filter = Filter {
            exclude: Some("%3"),
            ..Filter::default()
        };
        let ids: Vec<String> = rows(&a_server(), &filter, &clock(&programs))
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["%1", "%2", "%4"]);
    }

    #[test]
    fn agents_only_keeps_the_configured_programs() {
        let programs = programs();
        let filter = Filter {
            only_agents: true,
            ..Filter::default()
        };
        let got: Vec<String> = rows(&a_server(), &filter, &clock(&programs))
            .into_iter()
            .map(|r| r.program)
            .collect();
        assert_eq!(got, vec!["claude", "codex"]);

        // And the list is the config's, not a compiled-in one.
        let mine = vec!["nvim".to_string()];
        let got: Vec<String> = rows(&a_server(), &filter, &clock(&mine))
            .into_iter()
            .map(|r| r.program)
            .collect();
        assert_eq!(got, vec!["nvim"]);
    }

    #[test]
    fn a_session_filter_keeps_only_that_session() {
        let programs = programs();
        let filter = Filter {
            session: Some("web"),
            ..Filter::default()
        };
        let ats: Vec<String> = rows(&a_server(), &filter, &clock(&programs))
            .into_iter()
            .map(|r| r.at)
            .collect();
        assert_eq!(ats, vec!["web:1.0", "web:1.1"]);
    }

    // ── the preview ─────────────────────────────────────────────────────────

    #[test]
    fn the_preview_is_the_last_lines_that_say_anything() {
        let screen = "one   \n\ntwo\n   \nthree\nfour\n\n\n";
        assert_eq!(preview_tail(screen, 15), "one\ntwo\nthree\nfour");
        assert_eq!(preview_tail(screen, 2), "three\nfour");
        assert_eq!(preview_tail("\n\n", 15), "");
    }
}
