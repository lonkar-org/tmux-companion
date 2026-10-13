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
//! An agent can say better than silence can. `tmux-companion agent asked`
//! from one of its hooks tells the daemon it stopped on a question, `done`
//! that it answered and is waiting for the next thing, `busy` that it is
//! working again; see `agent.rs`. A report wins over the window's quiet time
//! for as long as the pane runs an agent. Between the two sits the terminal
//! bell: an agent that rings it when it needs you sets `#{window_bell_flag}`
//! until the window is visited, and that reads as asked.
//!
//! The one `list-panes -a` here is shared with the bar's `agents` segment,
//! which counts the same rows the picker shows, so the two cannot disagree
//! about what an agent is or when one is waiting.
//!
//! The picker runs in the **client**, and talks to tmux directly: there is
//! nothing for a daemon to cache in a list somebody is about to pick from,
//! and the picker needs the terminal the daemon does not have. It asks the
//! daemon for one thing only, the reports, since those live nowhere else.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The colour of a waiting agent's row, and of the waiting count on the bar.
///
/// An orange between the battery's yellow and its red: something that wants a
/// look, not something that has gone wrong. Fixed like the other segments'
/// colours rather than taken from the theme, which paints sessions, not the
/// bar.
pub const WAITING_COLOUR: &str = "colour214";

/// The colour of the busy count on the bar: a green that says nothing is
/// needed, distinct from the grey of the total so the number reads at a
/// glance.
pub const BUSY_COLOUR: &str = "colour114";

/// What an agent says about itself, through a hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Report {
    /// Working: a prompt was sent, or a tool just ran.
    Busy,
    /// Stopped on a question or a permission prompt.
    Asked,
    /// Answered, and waiting for the next prompt.
    Done,
}

impl Report {
    /// The word on the command line and in the column.
    pub fn word(self) -> &'static str {
        match self {
            Report::Busy => "busy",
            Report::Asked => "asked",
            Report::Done => "done",
        }
    }

    /// The report a word names, when it does.
    pub fn from_word(w: &str) -> Option<Self> {
        Some(match w.trim() {
            "busy" => Report::Busy,
            "asked" => Report::Asked,
            "done" => Report::Done,
            _ => return None,
        })
    }
}

/// One report, with when it arrived, as the daemon keeps it per pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reported {
    /// What the agent said.
    pub state: Report,
    /// When, in unix seconds.
    pub at: u64,
}

/// The reports the daemon holds, keyed by pane id.
pub type Reports = HashMap<String, Reported>;

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
    /// `#{window_bell_flag}`: the window rang the bell and nobody has visited
    /// it since. Needs `monitor-bell on`, which is tmux's default.
    pub bell: bool,
    /// `#{session_id}`, the `$N` a popup's `$TMUX` ends with.
    pub session_id: String,
    /// `#{@theme-color-main-1}` as this pane resolves it, empty when no theme
    /// set it.
    pub accent: String,
    /// `#{pane_pid}`: the process the pane started with, which holds its
    /// terminal. 0 when the listing did not say.
    pub pid: u32,
    /// The name the program in front was started by, when `command` hides
    /// it and the process could be read: `claude` for `2.1.296`, `gemini`
    /// for `node`. Filled by [`list`]; see `identity.rs`.
    pub identity: Option<String>,
    /// Whether the session is a grouped one another machine's mirror
    /// attached to (`@tc-mirror`): it lists every pane of the session it is
    /// grouped with a second time. Its active pane is what that machine's
    /// bar shows; counted as an agent, it would be every agent twice.
    pub mirrored: bool,
    /// `#{@theme-file}`: the stem of the theme file the session was painted
    /// from, empty before a theme or from a listing older than it.
    pub theme_file: String,
}

impl Pane {
    /// What the pane runs, by the best name known for it: the name it was
    /// started by when that was read, tmux's otherwise. Every question
    /// about whether a pane is an agent, and what it is called, asks this.
    pub fn name(&self) -> &str {
        self.identity.as_deref().unwrap_or(&self.command)
    }
}

/// The `-F` string the listing asks for.
///
/// `#{host}` rides along so the title can be compared against tmux's own
/// default for it without a second call: a pane whose title is the hostname
/// has no title worth showing.
pub fn pane_format() -> &'static str {
    "#{session_name}\t#{window_index}\t#{window_name}\t#{pane_index}\t#{pane_id}\t\
     #{pane_current_command}\t#{pane_current_path}\t#{pane_title}\t#{pane_active}\t\
     #{window_active}\t#{session_attached}\t#{window_activity}\t#{pane_in_mode}\t#{host}\t\
     #{window_bell_flag}\t#{session_id}\t#{@theme-color-main-1}\t#{pane_pid}\t#{@tc-mirror}\t#{@theme-file}"
}

/// How many fields [`pane_format`] produces, and how many a listing from
/// before each of the last two, `@tc-mirror` and `@theme-file`, has.
const FIELDS: usize = 20;
const FIELDS_BEFORE_THEME_FILE: usize = 19;
const FIELDS_BEFORE_MIRRORS: usize = 18;

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
            if ![FIELDS, FIELDS_BEFORE_THEME_FILE, FIELDS_BEFORE_MIRRORS].contains(&f.len()) {
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
                bell: on(f[14]),
                session_id: f[15].trim().to_string(),
                accent: f[16].trim().to_string(),
                pid: f[17].trim().parse().unwrap_or(0),
                identity: None,
                mirrored: f.get(18).is_some_and(|m| !m.trim().is_empty()),
                theme_file: f.get(19).map(|t| t.trim().to_string()).unwrap_or_default(),
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
///
/// A command that is nothing but a version number counts too. claude sets
/// its process title to its version, so tmux reports its pane as `2.1.283`
/// and no name in `[agents] programs` ever matches it; on the laptop this
/// was written on, that was the only agent running and the bar said none.
/// Nothing else names a process that way.
///
/// So does a program's name with a release version after it. grok installs
/// each release as `grok-1.0.50-macos-x86_64` and runs it through a symlink
/// called `grok`; the kernel names the process after the file, cut to 15
/// bytes, so tmux reports `grok-1.0.50-mac` and an exact match never saw it.
pub fn is_agent(command: &str, programs: &[String]) -> bool {
    programs.iter().any(|p| names(command, p)) || is_version_name(command)
}

/// Whether `command`, as tmux reports it, is `program`: the name itself, or
/// the name with a release version after it (see [`versioned_name`]).
pub fn names(command: &str, program: &str) -> bool {
    command == program || versioned_name(command) == Some(program)
}

/// `grok-1.0.50-mac` is `grok`: the part before a hyphen that starts a
/// release version, a digit with a dot after the digits. `None` for a command
/// with no such hyphen, and for `gcc-13`, where the number is a series rather
/// than a release.
pub fn versioned_name(command: &str) -> Option<&str> {
    let c = command.trim();
    c.match_indices('-').find_map(|(i, _)| {
        let rest = &c[i + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        (i > 0 && digits > 0 && rest.as_bytes().get(digits) == Some(&b'.')).then(|| &c[..i])
    })
}

/// `2.1.283`, `1.0`: digits and dots and nothing else, with at least one dot.
pub fn is_version_name(command: &str) -> bool {
    let c = command.trim();
    !c.is_empty()
        && c.contains('.')
        && c.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && !c.starts_with('.')
        && !c.ends_with('.')
        && !c.contains("..")
}

/// What a pane is doing, as far as the daemon can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// In copy mode: somebody is looking at it, whatever it is doing.
    Reading,
    /// An agent that is working: it said so, or it drew something recently.
    Busy,
    /// An agent that stopped on a question this long ago, in seconds: it
    /// said so through a hook, or it rang the bell.
    Asked(u64),
    /// An agent that said it answered, this long ago, and waits for the next
    /// prompt.
    Done(u64),
    /// An agent that has been quiet this long, in seconds, with nothing
    /// better to go on.
    Waiting(u64),
    /// Anything else that drew something recently.
    Active,
    /// Anything else that has been quiet this long, in seconds.
    Idle(u64),
}

impl State {
    /// An agent that has stopped and may be waiting on a person: asked,
    /// done, or quiet.
    pub fn is_waiting(self) -> bool {
        matches!(self, State::Asked(_) | State::Done(_) | State::Waiting(_))
    }

    /// An agent that is working.
    pub fn is_busy(self) -> bool {
        matches!(self, State::Busy)
    }

    /// An agent that stopped on a question, which is the one that needs an
    /// answer rather than a next prompt.
    pub fn is_asked(self) -> bool {
        matches!(self, State::Asked(_))
    }

    /// How long a stopped agent has waited, in seconds.
    pub fn waited(self) -> Option<u64> {
        match self {
            State::Asked(s) | State::Done(s) | State::Waiting(s) => Some(s),
            _ => None,
        }
    }

    /// The first word, for a column that sorts or colours by it.
    pub fn word(self) -> &'static str {
        match self {
            State::Reading => "reading",
            State::Busy => "busy",
            State::Asked(_) => "asked",
            State::Done(_) => "done",
            State::Waiting(_) => "waiting",
            State::Active => "active",
            State::Idle(_) => "idle",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            State::Asked(secs) | State::Done(secs) | State::Waiting(secs) | State::Idle(secs) => {
                write!(f, "{} {}", self.word(), age(*secs))
            }
            _ => f.write_str(self.word()),
        }
    }
}

/// Classify one pane at a given moment.
///
/// Copy mode wins over everything: a pane somebody is reading is not one that
/// needs pointing out. For an agent, what it reported wins next: a hook that
/// said `asked` or `done` knows more than a quiet window does, and one that
/// said `busy` covers the silent minute of a long tool. Then the bell: a
/// window that rang it and has not been visited since is an agent that wanted
/// somebody. Last, the window's quiet time against `waiting_secs`, with
/// agents and everything else getting different words for the same two
/// answers, because "idle" is what a shell is most of the day and "waiting"
/// is what an agent is when it may have asked you something.
pub fn state(
    pane: &Pane,
    agent: bool,
    report: Option<Reported>,
    now: u64,
    waiting_secs: u64,
) -> State {
    if pane.in_mode {
        return State::Reading;
    }
    let quiet = now.saturating_sub(pane.activity);
    if agent {
        match report {
            Some(Reported {
                state: Report::Busy,
                ..
            }) => return State::Busy,
            Some(Reported {
                state: Report::Asked,
                at,
            }) => return State::Asked(now.saturating_sub(at)),
            Some(Reported {
                state: Report::Done,
                at,
            }) => return State::Done(now.saturating_sub(at)),
            None => {}
        }
        if pane.bell {
            return State::Asked(quiet);
        }
    }
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
    let command = program_name(pane.name());
    let title = pane.title.trim();
    if title.is_empty() || title == pane.host || title == pane.command || title == command {
        return command.to_string();
    }
    format!("{command} \u{2014} {title}")
}

/// The name a command is shown by: itself, unless it is only a version
/// number, which is claude, the one program that names its process that way.
/// `2.1.283 has waited 5m` told nobody which program had. A name with a
/// release version after it, `grok-1.0.50-mac`, is shown without the version,
/// which tmux has cut off part way through anyway.
pub fn program_name(command: &str) -> &str {
    if is_version_name(command) {
        VERSION_NAMED
    } else {
        versioned_name(command).unwrap_or(command)
    }
}

/// What a process named by its version is called in a row or a nudge.
pub const VERSION_NAMED: &str = "claude";

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
    /// What the agents reported, from the daemon; empty when it was not
    /// asked.
    pub reports: &'a Reports,
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
            let agent = is_agent(p.name(), clock.programs);
            if filter.only_agents && !agent {
                return None;
            }
            Some(Row {
                at: format!("{}:{}.{}", p.session, p.window_index, p.pane_index),
                program: program(p),
                state: state(
                    p,
                    agent,
                    clock.reports.get(&p.id).copied(),
                    clock.now,
                    clock.waiting_secs,
                ),
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
///
/// A pane whose command hides the program (`node`, `2.1.296`) gets the name
/// the program was started by, read from the process; see `identity.rs` for
/// what that costs and why only those panes pay it.
pub async fn list() -> Vec<Pane> {
    let mut panes =
        parse(&crate::cli::tmux_capture(&["list-panes", "-a", "-F", pane_format()]).await);
    identify(&mut panes);
    panes
}

/// Every pane and every session, in one tmux call: the session list rides
/// after the pane list, its lines marked so neither parser reads the other's.
pub async fn list_with_sessions() -> (Vec<Pane>, crate::link::Listing) {
    let text = crate::cli::tmux_capture(&[
        "list-panes",
        "-a",
        "-F",
        pane_format(),
        ";",
        "list-sessions",
        "-F",
        crate::link::SESSION_FORMAT,
    ])
    .await;
    let mut panes = parse(&text);
    identify(&mut panes);
    (panes, crate::link::parse_listing(&text))
}

/// Fill in each pane's [`Pane::identity`].
pub fn identify(panes: &mut [Pane]) {
    for p in panes {
        p.identity = crate::identity::of(p.pid, &p.command);
    }
}

/// What the agents have reported, from the daemon. Empty when it cannot be
/// asked, which leaves every state to the window's quiet time.
pub async fn reports() -> Reports {
    crate::client::send(crate::proto::Request::raw(
        "__reports",
        serde_json::Value::Null,
    ))
    .await
    .ok()
    .filter(|r| r.error.is_none())
    .and_then(|r| serde_json::from_str(&r.output).ok())
    .unwrap_or_default()
}

/// The pane list, from the daemon's last read when that is younger than
/// `max_age`, and read now otherwise.
///
/// Three loops (inbox, notify, journal) and the agents segment each ran
/// their own `list-panes -a` on their own timer, so the daemon forked about
/// one every second for one answer. Each now passes its own interval: a
/// list read by another loop since its last turn is as fresh as one it would
/// have read itself. Read outside the lock, stored under it.
pub async fn shared(
    state: &std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
    max_age: std::time::Duration,
) -> Vec<Pane> {
    if let Some(panes) = state.lock().await.panes_fresh(max_age) {
        return panes;
    }
    let (panes, listing) = list_with_sessions().await;
    let mut st = state.lock().await;
    st.panes_store(panes.clone());
    st.sessions_store(listing);
    panes
}

/// The colour a session's visible pane resolves, or any of its panes' when
/// none is visible; `None` for a session not in the list.
pub fn accent_in(panes: &[Pane], session_id: &str) -> Option<String> {
    let mine = || panes.iter().filter(|p| p.session_id == session_id);
    mine()
        .find(|p| p.visible)
        .or_else(|| mine().next())
        .map(|p| p.accent.clone())
}

/// How old a pane list the accent is answered from. Push mode reads one every
/// second and the inbox every two, so this is a list a loop already paid for;
/// past it the daemon reads its own rather than answer from an old one.
const ACCENT_TRUST: std::time::Duration = std::time::Duration::from_secs(3);

/// A session's theme colour, as its visible pane resolves it, for a picker
/// that would otherwise start a tmux process to ask.
///
/// `None` is a session the listing does not have, which a fresh read is
/// tried for once: a picker opened in a session made a moment ago.
pub async fn accent_of(
    state: &std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
    session_id: &str,
) -> Option<String> {
    if let Some(accent) = accent_in(&shared(state, ACCENT_TRUST).await, session_id) {
        return Some(accent);
    }
    let panes = list().await;
    let accent = accent_in(&panes, session_id);
    state.lock().await.panes_store(panes);
    accent
}

/// The last lines of one pane's screen, as the preview shows them.
pub async fn tail_of(id: &str) -> String {
    let screen =
        crate::cli::tmux_capture(&["capture-pane", "-p", "-t", id, "-S", CAPTURE_LINES]).await;
    preview_tail(&screen, PREVIEW_LINES)
}

/// [`tail_of`] outside the runtime, for the picker's lazy preview.
fn tail_of_sync(id: &str) -> String {
    let screen =
        crate::cli::tmux_capture_sync(&["capture-pane", "-p", "-t", id, "-S", CAPTURE_LINES]);
    preview_tail(&screen, PREVIEW_LINES)
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

/// The icon and tone a picker row's state gets: the bar's waiting colour on
/// the state of an agent that asked or went quiet, the busy colour on one that
/// said it is working, and grey for a `done` agent or a pane gone quiet, which
/// need no more than the row.
///
/// The tone goes on the state cell and the icon, never the whole row: the
/// path beside a waiting agent is not waiting.
pub fn state_look(state: State) -> (&'static str, crate::picker::Tone) {
    use crate::picker::Tone;
    use crate::tmux::icons;
    match state {
        State::Asked(_) => word_look("asked"),
        State::Waiting(_) => word_look("waiting"),
        State::Busy => word_look("busy"),
        State::Done(_) => word_look("done"),
        State::Reading => word_look("reading"),
        State::Active => (icons::PANE, Tone::Plain),
        State::Idle(_) => (icons::PANE, Tone::Dim),
    }
}

/// The icon and tone of an agent state by its word, the one place they are
/// decided, so the bar, the panes picker, the inbox, the brief and the
/// journal draw a state the same way: busy green with the bar's busy mark,
/// asked bright red with a question mark, done steel blue with a check,
/// waiting amber with the hourglass, reading mauve with tmux's own mark.
pub fn word_look(word: &str) -> (&'static str, crate::picker::Tone) {
    use crate::picker::Tone;
    use crate::tmux::icons;
    match word {
        "busy" => (icons::BUSY, Tone::Busy),
        "asked" => (icons::ASKED, Tone::Asked),
        "done" => (icons::CHECK, Tone::Done),
        "reading" => (icons::READING, Tone::Reading),
        _ => (icons::WAITING, Tone::Waiting),
    }
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

    let chrome = crate::picker::Chrome {
        title: if agents { "[ Agents ]" } else { "[ Panes ]" }.into(),
        icon: if agents {
            crate::tmux::icons::AGENT
        } else {
            crate::tmux::icons::PANE
        }
        .into(),
        footer: "enter jump   ctrl-a clear   esc cancel".into(),
        preview_title: "[ Screen ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Panes);
    // On screen before the listing: under load the two reads below took
    // 22 ms, about a third of the time to the first frame.
    let loading = (!print).then(|| crate::picker::loading(&chrome, ""));

    let (panes, reports) = tokio::join!(list(), reports());
    let filter = Filter {
        exclude: here.as_deref(),
        session: target.as_deref(),
        only_agents: agents,
    };
    let clock = Clock {
        now: now_secs(),
        waiting_secs: config.agents.waiting_secs,
        programs: &config.agents.programs,
        reports: &reports,
        home: &home,
    };
    let rows = rows(&panes, &filter, &clock);

    if rows.is_empty() {
        // The frame comes down before the message, which goes to stderr and
        // to tmux, neither of them a screen in raw mode.
        drop(loading);
        crate::picker::say_nothing_to_show(
            if agents {
                "no agent panes running"
            } else {
                "no panes to show"
            },
            print,
        )
        .await;
        return Ok(());
    }

    if print {
        for r in &rows {
            println!("{}\t{}\t{}\t{}\t{}", r.at, r.program, r.state, r.cwd, r.id);
        }
        return Ok(());
    }

    let items: Vec<crate::picker::Item> = rows
        .iter()
        .map(|r| {
            use crate::picker::{Cell, Tone};
            let state = r.state.to_string();
            // The same colours the bar uses, so the row that wants you is
            // the one that stands out here too.
            let (icon, tone) = state_look(r.state);
            // One capture when the row is selected, rather than one per pane
            // before the first frame, all at once against a tmux server that
            // is as short of CPU as this process is.
            let id = r.id.clone();
            crate::picker::Item::with_lazy_preview(
                format!("{} {} {} {}", r.at, r.program, state, r.cwd),
                move || tail_of_sync(&id),
            )
            .in_cells(vec![
                Cell::strong(&r.at),
                Cell::plain(&r.program),
                Cell::new(state, if tone == Tone::Plain { Tone::Dim } else { tone }),
                Cell::dim(&r.cwd),
            ])
            .with_icon(icon, tone)
        })
        .collect();

    let picked = crate::picker::run(items, "", &chrome)?;
    drop(loading);
    if let Some(index) = picked
        && let Some(row) = rows.get(index)
    {
        jump(&row.id).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grouped_session_another_machine_attached_is_marked() {
        let line = |session: &str, mirror: &str| {
            format!(
                "{session}\t1\tai\t0\t%7\tclaude\t/w\th\t1\t1\t1\t5\t0\th\t0\t$1\t\t42\t{mirror}"
            )
        };
        let text = format!("{}\n{}\n", line("api", ""), line("api~mac", "mac"));
        let panes = parse(&text);
        assert_eq!(panes.len(), 2, "{panes:?}");
        assert_eq!(
            panes.iter().map(|p| p.mirrored).collect::<Vec<_>>(),
            [false, true]
        );
        let before = "api\t1\tai\t0\t%7\tclaude\t/w\th\t1\t1\t1\t5\t0\th\t0\t$1\t\t42";
        assert_eq!(parse(before).len(), 1, "a listing from before the field");
    }

    #[test]
    fn a_process_named_by_its_version_is_an_agent() {
        // claude's pane reports `2.1.283`; nothing in the list says that.
        let programs = vec!["claude".to_string()];
        assert!(is_agent("claude", &programs));
        assert!(is_agent("2.1.283", &programs));
        assert!(is_agent("1.0", &programs));
        assert!(!is_agent("zsh", &programs));
        assert!(!is_agent("3", &programs), "a bare number is not a version");
        assert!(!is_agent("1..2", &programs));
        assert!(!is_agent(".1", &programs));
        assert!(!is_agent("v1.2", &programs));
    }

    #[test]
    fn a_program_run_from_a_versioned_file_is_an_agent() {
        // grok runs `grok-1.0.50-macos-x86_64` through a symlink named
        // `grok`, and tmux reports the kernel's name for it, cut to 15 bytes:
        // `grok-1.0.50-mac` on macOS, `grok-1.0.50-lin` on Linux.
        let programs: Vec<String> = ["grok", "cursor-agent"].map(String::from).to_vec();
        assert!(is_agent("grok", &programs));
        assert!(is_agent("grok-1.0.50-mac", &programs));
        assert!(is_agent("grok-1.0.50-lin", &programs));
        assert!(is_agent("cursor-agent-1.", &programs), "cut off at the dot");
        assert!(!is_agent("grokky-1.0.50", &programs));
        assert!(!is_agent("grok-macos-x86_64", &programs), "no version");
        assert!(!is_agent("cursor", &programs));
        assert!(
            !is_agent("gcc-13", &["gcc".to_string()]),
            "a series, not a release"
        );
        assert!(!is_agent("-1.0", &programs));
        assert_eq!(program_name("grok-1.0.50-mac"), "grok");
        assert_eq!(program_name("cursor-agent-1.2"), "cursor-agent");
        assert_eq!(program_name("gcc-13"), "gcc-13");
        assert_eq!(program_name("zsh"), "zsh");
    }

    #[test]
    fn a_session_answers_with_its_visible_panes_colour() {
        let listing = "\
api\t1\tedit\t0\t%1\tzsh\t/a\tlaptop\t0\t1\t1\t0\t0\tlaptop\t0\t$1\tcolour60\t0
api\t2\tedit\t0\t%2\tzsh\t/a\tlaptop\t1\t1\t1\t0\t0\tlaptop\t0\t$1\tcolour99\t0
web\t1\tedit\t0\t%3\tzsh\t/w\tlaptop\t1\t1\t0\t0\t0\tlaptop\t0\t$2\t\t0
";
        let panes = parse(listing);
        assert_eq!(accent_in(&panes, "$1").as_deref(), Some("colour99"));
        assert_eq!(
            accent_in(&panes, "$2").as_deref(),
            Some(""),
            "known, no theme"
        );
        assert_eq!(accent_in(&panes, "$9"), None, "a session it has not seen");
    }

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
            "{session}\t{window}\tedit\t{pane}\t{id}\t{command}\t{path}\t{title}\t1\t1\t1\t{activity}\t{}\tlaptop\t0\t$0\t\t0\n",
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
        assert!(!p.bell);
        assert_eq!(p.pid, 0);
        assert_eq!(p.identity, None);
        let with_pid = format!(
            "{}4242\n",
            line("api", 2, 1, "%7", "zsh", "/a", "x", 0, false).trim_end_matches("0\n")
        );
        assert_eq!(parse(&with_pid)[0].pid, 4242);
    }

    #[test]
    fn a_pane_is_the_program_it_was_started_as_when_tmux_names_something_else() {
        // tmux says `node` for gemini and `2.1.296` for claude; the process
        // says which program it is, and that name is the one asked about.
        let mut gemini = pane("node", 0, false);
        assert!(!is_agent(gemini.name(), &programs()));
        gemini.identity = Some("gemini".into());
        assert!(is_agent(gemini.name(), &programs()));
        assert_eq!(program(&gemini), "gemini");
        let mut claude = pane("2.1.296", 0, false);
        claude.identity = Some("claude".into());
        assert_eq!(program(&claude), "claude");
        let mut server = pane("node", 0, false);
        server.identity = Some("vite".into());
        assert!(!is_agent(server.name(), &programs()));
        assert_eq!(program(&server), "vite");
        let rows = rows(
            &[gemini, server],
            &Filter {
                only_agents: true,
                ..Filter::default()
            },
            &Clock {
                now: 0,
                waiting_secs: 10,
                programs: &programs(),
                reports: &Reports::new(),
                home: "/home/me",
            },
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].program, "gemini");
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
        let hidden = "api\t2\tedit\t1\t%7\tclaude\t/a\tlaptop\t1\t0\t1\t0\t0\tlaptop\t0\t$0\t\t0\n";
        assert!(!parse(hidden)[0].visible, "window not active");
        let detached =
            "api\t2\tedit\t1\t%7\tclaude\t/a\tlaptop\t1\t1\t0\t0\t0\tlaptop\t0\t$0\t\t0\n";
        assert!(!parse(detached)[0].visible, "nobody attached");
    }

    // ── state ───────────────────────────────────────────────────────────────

    #[test]
    fn an_agent_that_drew_recently_is_busy_and_a_quiet_one_is_waiting() {
        let now = 1_000;
        assert_eq!(
            state(&pane("claude", 995, false), true, None, now, 10),
            State::Busy
        );
        assert_eq!(
            state(&pane("claude", 800, false), true, None, now, 10),
            State::Waiting(200)
        );
        // Exactly the threshold is waiting: ten seconds of nothing is what
        // the setting says counts.
        assert_eq!(
            state(&pane("claude", 990, false), true, None, now, 10),
            State::Waiting(10)
        );
    }

    #[test]
    fn a_report_wins_over_the_window_and_the_bell_over_silence() {
        let now = 1_000;
        let said = |state, at| Some(Reported { state, at });
        // Quiet for 200s but it said busy: a long silent tool, not a stop.
        let s = state(
            &pane("claude", 800, false),
            true,
            said(Report::Busy, 900),
            now,
            10,
        );
        assert_eq!(s, State::Busy);
        assert!(s.is_busy() && !s.is_waiting());
        // Drawing away, but it said asked: the wait is counted from the hook.
        let s = state(
            &pane("claude", 999, false),
            true,
            said(Report::Asked, 940),
            now,
            10,
        );
        assert_eq!(s, State::Asked(60));
        assert!(s.is_asked() && s.is_waiting() && !s.is_busy());
        let s = state(
            &pane("claude", 999, false),
            true,
            said(Report::Done, 700),
            now,
            10,
        );
        assert_eq!(s, State::Done(300));
        assert!(s.is_waiting() && !s.is_asked());
        // A report about a shell means nothing: the pane is not an agent.
        assert_eq!(
            state(
                &pane("zsh", 999, false),
                false,
                said(Report::Asked, 900),
                now,
                10
            ),
            State::Active
        );
        // The bell: drawing recently, but it rang and nobody came.
        let mut rang = pane("claude", 999, false);
        rang.bell = true;
        assert_eq!(state(&rang, true, None, now, 10), State::Asked(1));
        // Unless it has since said it is busy again.
        assert_eq!(
            state(&rang, true, said(Report::Busy, 999), now, 10),
            State::Busy
        );
        // Copy mode still wins over everything.
        let mut read = pane("claude", 800, true);
        read.bell = true;
        assert_eq!(
            state(&read, true, said(Report::Asked, 900), now, 10),
            State::Reading
        );
    }

    #[test]
    fn the_report_words_round_trip_and_colour_the_rows() {
        for r in [Report::Busy, Report::Asked, Report::Done] {
            assert_eq!(Report::from_word(r.word()), Some(r));
        }
        assert_eq!(Report::from_word("thinking"), None);
        use crate::picker::Tone;
        assert_eq!(state_look(State::Asked(1)).1, Tone::Asked);
        assert_eq!(state_look(State::Waiting(1)).1, Tone::Waiting);
        assert_eq!(state_look(State::Busy).1, Tone::Busy);
        assert_eq!(state_look(State::Done(1)).1, Tone::Done);
        assert_eq!(state_look(State::Reading).1, Tone::Reading);
        assert_ne!(
            state_look(State::Asked(1)).0,
            state_look(State::Waiting(1)).0
        );
        assert_eq!(state_look(State::Idle(1)).1, Tone::Dim);
        // Asked and busy must not share an icon: the colour is not the only
        // way to tell them apart, which matters under NO_COLOR.
        assert_ne!(state_look(State::Asked(1)).0, state_look(State::Busy).0);
    }

    #[test]
    fn everything_else_gets_the_plain_words() {
        let now = 1_000;
        assert_eq!(
            state(&pane("zsh", 999, false), false, None, now, 10),
            State::Active
        );
        assert_eq!(
            state(&pane("zsh", 0, false), false, None, now, 10),
            State::Idle(1_000)
        );
    }

    #[test]
    fn copy_mode_is_reading_whatever_else_is_true() {
        // Somebody is looking at it, so it is not a pane that needs pointing
        // out, and it is not counted as waiting on the bar either.
        let s = state(&pane("claude", 0, true), true, None, 1_000, 10);
        assert_eq!(s, State::Reading);
        assert!(!s.is_waiting());
    }

    #[test]
    fn a_clock_that_runs_backwards_does_not_panic() {
        // tmux's clock and this process's are two reads of the same thing,
        // and a window that drew between them reports a future time.
        assert_eq!(
            state(&pane("zsh", 2_000, false), false, None, 1_000, 10),
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
        assert_eq!(State::Asked(65).to_string(), "asked 1m");
        assert_eq!(State::Done(5).to_string(), "done 5s");
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
        // A process named by its version is shown by the name it has.
        p.command = "2.1.283".into();
        p.title = "\u{2733} fix the build".into();
        assert_eq!(program(&p), "claude \u{2014} \u{2733} fix the build");
        p.title = "laptop".into();
        assert_eq!(program(&p), "claude");
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
        static NONE: std::sync::LazyLock<Reports> = std::sync::LazyLock::new(Reports::new);
        Clock {
            now: 1_000,
            waiting_secs: 10,
            programs,
            reports: &NONE,
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
