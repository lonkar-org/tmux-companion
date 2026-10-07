//! One screen with the state of the server, for the moment you sit down.
//!
//! Everything on it exists as a command already: `inbox` for the agents
//! waiting, `doctor` for the health reasons, `sessions idle` for the sessions
//! nobody has touched, `sessions list` for the last snapshot. This is the
//! version that answers "what needs me" in one look, and the `--hook` form for
//! `client-attached` opens it only when the answer is not "nothing".
//!
//! It is also somewhere to act from. Every row a key can go to has a number,
//! and single keys acknowledge health, close an idle session, save a snapshot,
//! start or end quiet hours, show the doctor report, or hand over to the inbox
//! or the journal. Going somewhere ends the brief; anything else says one line
//! about what it did and draws the brief again. The screen-reader half,
//! the same actions as a typed answer to a question, waits for
//! screen-reader-mode: `docs/backlog/brief-keys-screen-reader.md`.

use std::io::IsTerminal;

use crate::inbox::Entry;
use crate::sessions::idle::IdleSession;

/// Days a session has to sit untouched before the brief lists it.
pub const IDLE_DAYS: u64 = 3;

/// What the brief has to say.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Brief {
    /// The agents waiting on you, longest wait first.
    pub waiting: Vec<Entry>,
    /// Why the health mark is up; empty means it is not.
    pub health: Vec<String>,
    /// Sessions with no client and nothing happening for [`IDLE_DAYS`].
    pub idle: Vec<IdleSession>,
    /// How many sessions the server has.
    pub sessions: usize,
    /// How many agents run, how many of those are busy, and how many are
    /// waiting.
    pub agents: (usize, usize, usize),
    /// The last snapshot's stamp and how long ago it was taken, when any.
    pub last_snapshot: Option<(String, u64)>,
    /// How many `setup` items are open, asked only in the week after this
    /// build first ran; `None` outside that week.
    pub setup_open: Option<usize>,
    /// The chunk clock's sitting in words, `38 min, 12 min for break`;
    /// empty when the clock is off or no sitting runs.
    pub sitting: String,
    /// How long quiet hours have left, `quiet for 40m more`, when they are
    /// on.
    pub quiet: Option<String>,
    /// How many journal events the last day holds, which is whether `j` has
    /// anything to open.
    pub journal_today: usize,
}

impl Brief {
    /// Whether the screen has anything that needs a person: an agent that
    /// asked or went quiet, or a health reason. One that said `done`
    /// answered and can wait; idle sessions and an old snapshot are not news.
    pub fn has_news(&self) -> bool {
        self.waiting.iter().any(|e| e.state != "done") || !self.health.is_empty()
    }
}

/// Unix seconds from a snapshot stamp, `20260926T133256`.
///
/// The inverse of `store::stamp_from`, days-from-civil in the other
/// direction; both are UTC, so no zone gets a say.
pub fn stamp_secs(stamp: &str) -> Option<u64> {
    let s = stamp.trim();
    if s.len() != 15 || s.as_bytes()[8] != b'T' {
        return None;
    }
    let n = |a: usize, b: usize| s[a..b].parse::<i64>().ok();
    let (y, m, d) = (n(0, 4)?, n(4, 6)?, n(6, 8)?);
    let (hh, mm, ss) = (n(9, 11)?, n(11, 13)?, n(13, 15)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// Agents listed in the brief before the rest are counted rather than shown:
/// the screen is for deciding what to do first, and a wall of rows is the
/// thing it exists to replace. They are longest-waiting first, so the ones
/// left out are the ones that have waited least.
pub const SHOWN: usize = 5;

/// The screen, as text.
///
/// Sections in the order they need answering: what is waiting on you, what
/// is wrong, what can go, what was said, then the one line of numbers.
pub fn render(b: &Brief, now: u64, home: &str) -> String {
    render_in(b, now, home, &crate::picker::Paint::plain())
}

/// The screen, painted: a heading with its icon for each section, and each
/// value in the tone that says what it is. With [`Paint::plain`] this is
/// exactly [`render`], which is what the tests and `--print` read.
///
/// Padding is done before the ink goes on, because a width counted over an
/// escape is a column that lands somewhere different on every line.
///
/// [`Paint::plain`]: crate::picker::Paint::plain
pub fn render_in(b: &Brief, now: u64, home: &str, paint: &crate::picker::Paint) -> String {
    drawn(b, now, home, paint, false)
}

/// The screen with a number in front of every row a key can go to, which is
/// what the brief draws when it waits for keys.
pub fn render_numbered(b: &Brief, now: u64, home: &str, paint: &crate::picker::Paint) -> String {
    drawn(b, now, home, paint, true)
}

/// [`render_in`] and [`render_numbered`], which differ only in the numbers.
fn drawn(b: &Brief, now: u64, home: &str, paint: &crate::picker::Paint, numbered: bool) -> String {
    use crate::picker::Tone;
    use crate::tmux::icons;
    let ink = |text: String, tone: Tone| paint.ink(&text, tone);
    // The numbers run on from the agents into the idle sessions, and are as
    // wide as the largest of them so the rows still line up.
    let width = targets(b).len().to_string().len();
    let number = |n: usize| {
        if numbered {
            format!("{}  ", ink(format!("{n:>width$}"), Tone::Strong))
        } else {
            String::new()
        }
    };
    let mut out = String::new();
    if b.waiting.is_empty() {
        out.push_str(&paint.icon(icons::CHECK, Tone::Ok));
        out.push_str("Nothing is waiting on you.\n");
    } else {
        out.push_str(&paint.heading(
            icons::WAITING,
            &format!("Waiting on you ({})", b.waiting.len()),
        ));
        out.push('\n');
        for (i, e) in b.waiting.iter().take(SHOWN).enumerate() {
            // Each state its one look, as everywhere: a `done` agent is here
            // to be read, not answered, so it is passive blue, not amber.
            let state = crate::panes::word_look(&e.state).1;
            out.push_str(&format!(
                "  {}{} {} {} {} {}\n",
                number(i + 1),
                ink(format!("{:<14}", e.at), Tone::Strong),
                ink(format!("{:<12}", e.program), Tone::Plain),
                ink(format!("{:<7}", e.state), state),
                ink(
                    format!("{:<5}", crate::panes::age(now.saturating_sub(e.since))),
                    Tone::Dim
                ),
                ink(e.question_line(), Tone::Quote)
            ));
        }
    }
    out.push('\n');
    if b.health.is_empty() {
        // The check, not the health mark: the mark is an alert, and an alert
        // drawn green beside `ok` says two things at once.
        out.push_str(&paint.icon(icons::CHECK, Tone::Ok));
        out.push_str(&format!("Health: {}\n", ink("ok".into(), Tone::Ok)));
    } else {
        out.push_str(&paint.heading(icons::HEALTH, "Health"));
        out.push('\n');
        for r in &b.health {
            // The bar draws the health mark in the waiting colour, so a
            // reason here is the same colour as the mark that sent you.
            out.push_str(&format!("  {}\n", ink(r.clone(), Tone::Waiting)));
        }
    }
    out.push('\n');
    if !b.idle.is_empty() {
        out.push_str(&paint.heading(
            icons::IDLE,
            &format!("Idle for {IDLE_DAYS}+ days ({})", b.idle.len()),
        ));
        out.push('\n');
        let first = shown_agents(b) + 1;
        for (i, s) in b.idle.iter().enumerate() {
            let cols = crate::sessions::idle::columns(s, home);
            let tones = [Tone::Strong, Tone::Dim, Tone::Plain, Tone::Dim];
            let cells: Vec<String> = cols
                .into_iter()
                .zip(tones)
                .map(|(c, t)| ink(c, t))
                .collect();
            out.push_str(&format!("  {}{}\n", number(first + i), cells.join("  ")));
        }
        out.push('\n');
    }
    let snapshot = match &b.last_snapshot {
        Some((_, age)) => format!("last snapshot {} ago", crate::panes::age(*age)),
        None => "no snapshot yet".to_string(),
    };
    // The numbers are the line; the words around them are grey. A waiting
    // count above zero is amber, because it is the one that wants you.
    let waiting = if b.agents.2 > 0 {
        Tone::Waiting
    } else {
        Tone::Strong
    };
    out.push_str(&format!(
        "{}{}{}{}{}{}{}{}\n",
        ink(b.sessions.to_string(), Tone::Strong),
        ink(
            format!(" session{}, ", if b.sessions == 1 { "" } else { "s" }),
            Tone::Dim
        ),
        ink(b.agents.0.to_string(), Tone::Strong),
        ink(
            format!(" agent{} (", if b.agents.0 == 1 { "" } else { "s" }),
            Tone::Dim
        ),
        ink(b.agents.1.to_string(), Tone::Strong),
        ink(" busy, ".into(), Tone::Dim),
        ink(b.agents.2.to_string(), waiting),
        ink(format!(" waiting), {snapshot}"), Tone::Dim),
    ));
    // Words rather than the bar's `50m+12`, so the same line is read aloud
    // and looked at; amber once it is past the break.
    if !b.sitting.is_empty() {
        let tone = if b.sitting.ends_with("passed break time") {
            Tone::Waiting
        } else {
            Tone::Dim
        };
        out.push_str(&format!("Sitting: {}\n", ink(b.sitting.clone(), tone)));
    }
    if let Some(quiet) = &b.quiet {
        out.push_str(&paint.icon(icons::QUIET, Tone::Dim));
        out.push_str(&format!(
            "Quiet hours: {}\n",
            ink(quiet.clone(), Tone::Plain)
        ));
    }
    if let Some(open) = b.setup_open.filter(|n| *n > 0) {
        out.push_str(&paint.icon(icons::SETUP, Tone::Dim));
        out.push_str(&format!(
            "{open} setup items open: {}\n",
            ink("tmux-companion setup".into(), Tone::Strong)
        ));
    }
    out
}

/// How many waiting agents carry a number: the ones shown.
fn shown_agents(b: &Brief) -> usize {
    b.waiting.len().min(SHOWN)
}

/// Where a number on the brief goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A waiting agent's pane, by `#{pane_id}`.
    Pane(String),
    /// An idle session, by name.
    Session(String),
}

/// Every numbered row, in number order: the shown agents, then the idle
/// sessions, each with the words that name it when it is picked.
pub fn targets(b: &Brief) -> Vec<(Target, String)> {
    b.waiting
        .iter()
        .take(SHOWN)
        .map(|e| {
            (
                Target::Pane(e.id.clone()),
                format!("{}, {}", e.at, e.program),
            )
        })
        .chain(
            b.idle
                .iter()
                .map(|s| (Target::Session(s.name.clone()), s.name.clone())),
        )
        .collect()
}

/// What a key, or a typed answer, asks the brief to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Go to a numbered row; the brief ends.
    Go(Target, String),
    /// Close an idle session, asking first.
    Close(String),
    /// Forget the failures the health mark is up for.
    Ack,
    /// Show the doctor report, then come back.
    Doctor,
    /// Hand over to the inbox picker; the brief ends.
    Inbox,
    /// Hand over to today's journal picker; the brief ends.
    Journal,
    /// Take a snapshot now.
    Save,
    /// An hour of quiet, or the end of it.
    Quiet,
    /// Close the brief.
    Leave,
    /// Nothing happens; this is said instead.
    Say(String),
}

/// A key the footer offers, because pressing it does something right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Numbers 1 to n go to a row.
    Go(usize),
    /// `c` and a number from the first to the last idle row closes it.
    Close(usize, usize),
    /// `h` acknowledges the health reasons.
    Ack,
    /// `d` shows the doctor report.
    Doctor,
    /// `i` opens the inbox.
    Inbox,
    /// `j` opens today's journal.
    Journal,
    /// `s` saves a snapshot.
    Save,
    /// `z` starts quiet hours, or ends them when the flag says they are on.
    Quiet(bool),
    /// `q` closes the brief.
    Leave,
}

/// The keys that do something on this brief, in the order the footer names
/// them. A key with nothing to act on is left out: `h` with health ok, `c`
/// with no idle session, `i` with nobody waiting.
pub fn offers(b: &Brief) -> Vec<Offer> {
    let total = targets(b).len();
    let mut out = Vec::new();
    if total > 0 {
        out.push(Offer::Go(total));
    }
    if !b.idle.is_empty() {
        out.push(Offer::Close(shown_agents(b) + 1, total));
    }
    if !b.health.is_empty() {
        out.push(Offer::Ack);
    }
    out.push(Offer::Doctor);
    if !b.waiting.is_empty() {
        out.push(Offer::Inbox);
    }
    if b.journal_today > 0 {
        out.push(Offer::Journal);
    }
    out.push(Offer::Save);
    out.push(Offer::Quiet(b.quiet.is_some()));
    out.push(Offer::Leave);
    out
}

/// A range of numbers as the footer writes it: `3`, or `1-3`.
fn range(first: usize, last: usize) -> String {
    if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    }
}

/// The drawn footer: each key and what it does, three spaces apart.
pub fn footer(b: &Brief) -> String {
    offers(b)
        .into_iter()
        .map(|o| match o {
            Offer::Go(n) => format!("{} go there", range(1, n)),
            Offer::Close(..) => "c close idle".into(),
            Offer::Ack => "h ack health".into(),
            Offer::Doctor => "d doctor".into(),
            Offer::Inbox => "i inbox".into(),
            Offer::Journal => "j journal".into(),
            Offer::Save => "s save".into(),
            Offer::Quiet(false) => "z quiet 1h".into(),
            Offer::Quiet(true) => "z end quiet".into(),
            Offer::Leave => "q close".into(),
        })
        .collect::<Vec<_>>()
        .join("   ")
}

/// The question screen-reader mode asks: the same keys as [`footer`], said
/// as a sentence.
pub fn question(b: &Brief) -> String {
    let said: Vec<String> = offers(b)
        .into_iter()
        .map(|o| match o {
            Offer::Go(1) => "1 to go there".into(),
            Offer::Go(n) => format!("a number from 1 to {n} to go there"),
            Offer::Close(f, l) if f == l => format!("c {f} to close the idle session"),
            Offer::Close(f, l) => {
                format!("c and a number from {f} to {l} to close an idle session")
            }
            Offer::Ack => "h to acknowledge the health reasons".into(),
            Offer::Doctor => "d for the doctor report".into(),
            Offer::Inbox => "i for the inbox".into(),
            Offer::Journal => "j for today's journal".into(),
            Offer::Save => "s to save a snapshot".into(),
            Offer::Quiet(false) => "z for an hour of quiet".into(),
            Offer::Quiet(true) => "z to end quiet hours".into(),
            Offer::Leave => "q to close".into(),
        })
        .collect();
    match said.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            format!("Type {}, or {last}:", rest.join(", "))
        }
        _ => format!("Type {}:", said.join("")),
    }
}

/// What an answer asks for, from the keys the drawn screen collected or the
/// line screen-reader mode read. Enter on its own closes, the way the brief
/// closed before it had keys.
pub fn choose(b: &Brief, answer: &str) -> Action {
    let answer = answer.trim().to_lowercase();
    let rows = targets(b);
    let is_number = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    let no_such = |n: &str| match rows.len() {
        0 => format!("There is no {n}: nothing here has a number."),
        total => format!("There is no {n}. The numbers go from 1 to {total}."),
    };
    match answer.as_str() {
        "" | "q" | "quit" | "exit" => return Action::Leave,
        "h" => {
            return if b.health.is_empty() {
                Action::Say("Health is ok; there is nothing to acknowledge.".into())
            } else {
                Action::Ack
            };
        }
        "d" => return Action::Doctor,
        "s" => return Action::Save,
        "z" => return Action::Quiet,
        "i" => {
            return if b.waiting.is_empty() {
                Action::Say("No agent is waiting on you.".into())
            } else {
                Action::Inbox
            };
        }
        "j" => {
            return if b.journal_today == 0 {
                Action::Say("Nothing in the journal today.".into())
            } else {
                Action::Journal
            };
        }
        "?" | "help" => return Action::Say(question(b)),
        _ => {}
    }
    if is_number(&answer) {
        let n: usize = answer.parse().unwrap_or(0);
        return match n.checked_sub(1).and_then(|i| rows.get(i)) {
            Some((target, label)) => Action::Go(target.clone(), label.clone()),
            None => Action::Say(no_such(&answer)),
        };
    }
    if let Some(rest) = answer.strip_prefix('c') {
        let rest = rest.trim();
        let first = shown_agents(b) + 1;
        let which = if b.idle.is_empty() {
            "There is no idle session to close.".to_string()
        } else if b.idle.len() == 1 {
            format!("c takes the idle session's number, {first}.")
        } else {
            format!(
                "c takes the number of an idle session, {first} to {}.",
                rows.len()
            )
        };
        if b.idle.is_empty() || !is_number(rest) {
            return Action::Say(which);
        }
        let n: usize = rest.parse().unwrap_or(0);
        return match n.checked_sub(1).and_then(|i| rows.get(i)) {
            Some((Target::Session(name), _)) => Action::Close(name.clone()),
            Some((Target::Pane(_), _)) => Action::Say(format!("{n} is an agent. {which}")),
            None => Action::Say(no_such(rest)),
        };
    }
    // Only what went wrong: the question, or the footer, is said next.
    Action::Say(format!("{answer} is not one of the keys; ? lists them."))
}

/// A key pressed on the drawn brief, as [`press`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A letter, a digit or anything else printable.
    Char(char),
    /// Enter.
    Enter,
    /// Escape.
    Esc,
    /// Backspace.
    Backspace,
    /// Ctrl-c.
    Interrupt,
}

/// What a key leaves: more to type, or an answer for [`choose`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pressed {
    /// Keys typed so far, waiting for the rest; empty is nothing pending.
    Pending(String),
    /// A whole answer.
    Answer(String),
}

/// One key on the drawn brief, given what is pending and how many rows have
/// numbers.
///
/// A letter answers at once. `c` waits for a number. A digit answers at once
/// unless another digit could still make a bigger number on the list, so with
/// nine rows or fewer every number is one key, and with twelve a `1` waits
/// for a second digit or enter. Escape drops what is pending, and with
/// nothing pending closes. With no idle session to close, `c` answers at once
/// so it can say so, rather than waiting for a number no row has.
pub fn press(pending: &str, key: Key, total: usize, can_close: bool) -> Pressed {
    match key {
        Key::Interrupt => Pressed::Answer("q".into()),
        Key::Esc if pending.is_empty() => Pressed::Answer("q".into()),
        Key::Esc => Pressed::Pending(String::new()),
        Key::Enter => Pressed::Answer(pending.to_string()),
        Key::Backspace => {
            let mut p = pending.to_string();
            p.pop();
            Pressed::Pending(p)
        }
        Key::Char(d) if d.is_ascii_digit() => {
            let typed = format!("{pending}{d}");
            let n: usize = typed.trim_start_matches('c').parse().unwrap_or(0);
            if n == 0 || n.saturating_mul(10) > total {
                Pressed::Answer(typed)
            } else {
                Pressed::Pending(typed)
            }
        }
        Key::Char('c') if pending.is_empty() && can_close => Pressed::Pending("c".into()),
        // A letter halfway through a number drops the number rather than
        // doing something nobody finished asking for.
        Key::Char(_) if !pending.is_empty() => Pressed::Pending(String::new()),
        Key::Char(c) => Pressed::Answer(c.to_string()),
    }
}

/// The line under the footer while keys are pending: what they are waiting
/// for.
pub fn pending_line(b: &Brief, pending: &str) -> Option<String> {
    if pending.is_empty() {
        return None;
    }
    let total = targets(b).len();
    Some(match pending.strip_prefix('c') {
        Some("") => format!(
            "close which? its number, {}   esc cancels",
            range(shown_agents(b) + 1, total)
        ),
        Some(n) => format!("close {n}: another digit, or enter takes it   esc cancels"),
        None => format!("{pending}: another digit, or enter takes it   esc cancels"),
    })
}

/// Ask the daemon and tmux for everything the screen shows.
pub async fn gather() -> Brief {
    let now = crate::panes::now_secs();
    let ask = |cmd: &'static str| async move {
        crate::client::send(crate::proto::Request::raw(cmd, serde_json::Value::Null))
            .await
            .ok()
            .filter(|r| r.error.is_none())
            .map(|r| r.output)
            .unwrap_or_default()
    };
    let chunk_status = async {
        let args = crate::proto::ChunkArgs {
            action: "status".to_string(),
            ..Default::default()
        };
        crate::client::send(crate::proto::Request::build("chunk", &args))
            .await
            .ok()
            .filter(|r| r.error.is_none())
            .map(|r| r.output)
            .unwrap_or_default()
    };
    let quiet = async {
        let args = crate::proto::QuietArgs { secs: None };
        crate::client::send(crate::proto::Request::build("__quiet", &args))
            .await
            .ok()
            .filter(|r| r.error.is_none())
            .map(|r| r.output.trim().to_string())
            .filter(|o| o.starts_with("quiet for"))
    };
    let (inbox_json, health_text, reports, idle, sessions_text, sitting, quiet) = tokio::join!(
        ask("__inbox"),
        ask("__health"),
        crate::panes::reports(),
        crate::sessions::idle::list(IDLE_DAYS),
        crate::cli::tmux_capture(&["list-sessions", "-F", "#{session_name}"]),
        chunk_status,
        quiet,
    );
    // The clock being off is said by `chunk status`, not on every brief.
    let sitting = if sitting.starts_with("The chunk clock is off") {
        String::new()
    } else {
        sitting
    };
    let waiting: Vec<Entry> = serde_json::from_str(&inbox_json).unwrap_or_default();
    let health = health_reasons(&health_text);
    let config = crate::cli::config_or_default();
    let panes = crate::panes::list().await;
    let sample = crate::segments::agents::count(
        &panes,
        &config.agents.programs,
        &reports,
        now,
        config.agents.waiting_secs,
    );
    let last_snapshot = crate::server::state_dir()
        .and_then(|d| crate::sessions::store::last_stamp_in(&d))
        .map(|stamp| {
            let age = stamp_secs(&stamp).map_or(0, |t| now.saturating_sub(t));
            (stamp, age)
        });
    // Only in the week after this build first ran, because after that the
    // list is something somebody has seen and decided about, and a line on
    // every attach would be the nag this screen exists not to be. The check
    // is one small file read; the count behind it is a handful of tmux calls
    // and is skipped outside the week.
    let setup_open = if crate::setup::build_is_fresh() {
        Some(crate::setup::count().await.0)
    } else {
        None
    };
    // The same day `journal` opens on, so `j` is offered only when it has
    // something to show.
    let journal_today = crate::journal::select(
        &crate::journal::read_all(),
        now.saturating_sub(crate::sessions::idle::DAY),
        None,
    )
    .len();
    Brief {
        waiting,
        health,
        idle,
        setup_open,
        sessions: sessions_text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        agents: (sample.total, sample.busy, sample.waiting),
        last_snapshot,
        sitting,
        quiet,
        journal_today,
    }
}

/// The health mark's reasons, less quiet hours.
///
/// The daemon puts quiet on the mark so a silent bar reads as chosen, but on
/// the brief it has a line of its own and a key, `z`, that ends it. Left in,
/// it would be said twice, offer `h` for something an acknowledgement cannot
/// clear, and open the popup on attach in the hours somebody asked not to be
/// nagged.
pub fn health_reasons(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("quiet for"))
        .map(str::to_string)
        .collect()
}

/// `brief`: print the screen; in a terminal, wait for keys and act on them.
///
/// `--hook` is the `client-attached` half: it opens the popup only when there
/// is news, and otherwise says nothing, so attaching to a quiet server stays
/// quiet. The binary is named by its own path for the reason `start --hook`
/// gives: a popup's shell has the tmux server's PATH.
pub async fn run(print: bool, hook: bool) -> anyhow::Result<()> {
    let b = gather().await;
    if hook {
        if !b.has_news() {
            return Ok(());
        }
        let me = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "tmux-companion".to_string());
        // The brief draws its own frame, so tmux's border goes where tmux
        // can leave it off, as it does for the pickers.
        let command = format!("{me} brief");
        let mut args = vec!["display-popup"];
        if crate::setup::catalog::popups_take_b() {
            args.push("-B");
        }
        args.extend(["-E", "-w", "90%", "-h", "75%", &command]);
        crate::cli::tmux(&args).await;
        return Ok(());
    }
    let home = std::env::var("HOME").unwrap_or_default();
    if print || !std::io::stdin().is_terminal() {
        let paint = crate::picker::Paint::for_stdout(print);
        print!("{}", render_in(&b, crate::panes::now_secs(), &home, &paint));
        return Ok(());
    }
    keyed(b, &home).await
}

/// How an action left the brief.
enum Done {
    /// It went somewhere, or handed over; the brief is over.
    Left,
    /// It did its thing here, and this is the line that says what happened.
    Stay(Option<String>),
}

/// The drawn brief: the screen, the footer, and one key at a time.
async fn keyed(mut b: Brief, home: &str) -> anyhow::Result<()> {
    let paint = crate::picker::Paint::for_stdout(false);
    let mut said: Option<String> = None;
    let mut pending = String::new();
    loop {
        draw(&b, home, &paint, said.as_deref(), &pending)?;
        let key = read_key()?;
        match press(&pending, key, targets(&b).len(), !b.idle.is_empty()) {
            Pressed::Pending(p) => {
                // A line about the key before is not about this one.
                said = None;
                pending = p;
            }
            Pressed::Answer(answer) => {
                pending.clear();
                said = match choose(&b, &answer) {
                    Action::Leave => return Ok(()),
                    Action::Say(text) => Some(text),
                    action => match act(action, &b, Some(&paint)).await? {
                        Done::Left => return Ok(()),
                        Done::Stay(line) => {
                            b = gather().await;
                            line
                        }
                    },
                };
            }
        }
    }
}

/// Clear the terminal and print the brief from the top, the footer, and the
/// line that says what the last key did or what it is waiting for.
fn draw(
    b: &Brief,
    home: &str,
    paint: &crate::picker::Paint,
    said: Option<&str>,
    pending: &str,
) -> anyhow::Result<()> {
    use crate::picker::Tone;
    let mut text = render_numbered(b, crate::panes::now_secs(), home, paint);
    // The keys go on the frame's own hint row, where every picker has them;
    // what the last key did stays under the brief, nearest where you read.
    text.push('\n');
    if let Some(line) = pending_line(b, pending) {
        text.push_str(&format!("{}\n", paint.ink(&line, Tone::Strong)));
    } else if let Some(said) = said {
        text.push_str(&format!("{}\n", paint.ink(said, Tone::Strong)));
    }
    // Framed the way the inbox is, since the brief opens from the same bar
    // and lists the same agents.
    let look = crate::cli::config_or_default()
        .picker
        .resolved(crate::config::Picker::Panes)
        .look;
    crate::picker::show_framed(
        &text,
        &footer(b),
        "[ Brief ]",
        crate::tmux::icons::HEALTH,
        &look,
        paint,
    )
}

/// One key, read in raw mode, with the terminal put back before returning so
/// everything printed between keys is ordinary lines.
fn read_key() -> anyhow::Result<Key> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use ratatui::crossterm::terminal;
    terminal::enable_raw_mode()?;
    let read = (|| -> anyhow::Result<Key> {
        loop {
            let Event::Key(k) = event::read()? else {
                continue;
            };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            return Ok(match k.code {
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Key::Interrupt,
                KeyCode::Char(c) => Key::Char(c),
                KeyCode::Enter => Key::Enter,
                KeyCode::Esc => Key::Esc,
                KeyCode::Backspace => Key::Backspace,
                _ => continue,
            });
        }
    })();
    terminal::disable_raw_mode()?;
    read
}

/// Do what was asked. `paint` is the drawn brief's; `None` is screen-reader
/// mode, which prints plain and asks in lines.
async fn act(
    action: Action,
    b: &Brief,
    paint: Option<&crate::picker::Paint>,
) -> anyhow::Result<Done> {
    use std::io::{BufRead, Write};
    Ok(match action {
        Action::Go(target, label) => {
            if paint.is_none() {
                println!("Going to {label}.");
            }
            match target {
                Target::Pane(id) => crate::panes::jump(&id).await,
                Target::Session(name) => {
                    if let Err(e) = crate::cli::focus_session(&name).await {
                        return Ok(Done::Stay(Some(format!("Could not go to {name}: {e}"))));
                    }
                }
            }
            Done::Left
        }
        Action::Close(name) => {
            println!();
            // The same question `sessions idle` asks, and the same close
            // after it, layout saved first.
            if !crate::setup::confirm(&format!("close {name}? its layout is saved first")) {
                return Ok(Done::Stay(Some(format!("Left {name} open."))));
            }
            println!("Closing {name}.");
            Done::Stay(Some(
                match crate::cli::run_close_project(Some(name.clone()), false, true).await {
                    Ok(()) => format!("Closed {name}; its layout is saved."),
                    Err(e) => format!("{name} is still open: {e}"),
                },
            ))
        }
        Action::Ack => {
            let args = crate::proto::HealthArgs { ack: true };
            let line =
                match crate::client::send(crate::proto::Request::build("health", &args)).await {
                    Ok(r) => match r.error {
                        Some(e) => format!("Not acknowledged: {e}"),
                        None => sentence(&r.output.lines().collect::<Vec<_>>().join("; ")),
                    },
                    Err(e) => format!("Not acknowledged: {e}"),
                };
            Done::Stay(Some(line))
        }
        Action::Doctor => {
            let report = match paint {
                Some(p) => {
                    use ratatui::crossterm::{cursor::MoveTo, execute, terminal};
                    execute!(
                        std::io::stdout(),
                        terminal::Clear(terminal::ClearType::All),
                        MoveTo(0, 0)
                    )?;
                    crate::doctor::report_in(p).await
                }
                None => crate::doctor::report().await,
            };
            print!("{report}");
            print!(
                "\n{}",
                if paint.is_some() {
                    "press enter to go back to the brief "
                } else {
                    "Enter goes back to the brief. "
                }
            );
            std::io::stdout().flush()?;
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            Done::Stay(None)
        }
        Action::Save => {
            let config = crate::cli::config_or_default();
            let line =
                match crate::sessions::timer::take_snapshot(&config.sessions, &[], false, false)
                    .await
                {
                    Ok(taken) => {
                        let snap = &taken.captured.snapshot;
                        let n =
                            |n: usize, what: &str| format!("{n} {what}{}", crate::cli::plural(n));
                        format!(
                            "Saved a snapshot: {}, {}, {}.",
                            n(snap.session.len(), "session"),
                            n(snap.window_count(), "window"),
                            n(snap.pane_count(), "pane")
                        )
                    }
                    Err(e) => format!("No snapshot taken: {e}"),
                };
            Done::Stay(Some(line))
        }
        Action::Quiet => {
            let on = b.quiet.is_none();
            let args = crate::proto::QuietArgs {
                secs: Some(if on { 3600 } else { 0 }),
            };
            let line =
                match crate::client::send(crate::proto::Request::build("__quiet", &args)).await {
                    Ok(r) if r.error.is_none() => {
                        let status = r.output.trim().to_string();
                        match (on, status.starts_with("quiet for")) {
                            (true, _) => format!("Quiet hours on: {status}."),
                            (false, false) => "Quiet hours off.".to_string(),
                            // Off from the timer, but a daily window from the
                            // config is still running, and only the config ends
                            // that.
                            (false, true) => {
                                format!("Still quiet from [quiet] daily: {status}.")
                            }
                        }
                    }
                    Ok(r) => format!("Quiet hours unchanged: {}", r.error.unwrap_or_default()),
                    Err(e) => format!("Quiet hours unchanged: {e}"),
                };
            Done::Stay(Some(line))
        }
        Action::Inbox => {
            crate::inbox::run(false).await?;
            Done::Left
        }
        Action::Journal => {
            crate::journal::run(false, None, 1).await?;
            Done::Left
        }
        Action::Leave => Done::Left,
        Action::Say(text) => Done::Stay(Some(text)),
    })
}

/// A report line as a sentence: capital first, full stop last.
fn sentence(text: &str) -> String {
    let t = text.trim();
    let mut chars = t.chars();
    let mut out: String = match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => return String::new(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_goes_back_to_the_seconds_it_came_from() {
        for secs in [
            0i64,
            951_868_800,
            1_709_210_096,
            1_790_067_600,
            1_790_427_176,
        ] {
            let stamp = crate::sessions::store::stamp_from(secs);
            assert_eq!(stamp_secs(&stamp), Some(secs as u64), "{stamp}");
        }
        assert_eq!(stamp_secs("2026-09-26"), None);
        assert_eq!(stamp_secs("20261326T000000"), None);
    }

    #[test]
    fn a_quiet_server_is_two_lines_and_no_news() {
        let b = Brief {
            sessions: 3,
            ..Default::default()
        };
        assert!(!b.has_news());
        let text = render(&b, 1000, "/home/me");
        assert!(text.starts_with("Nothing is waiting on you.\n"), "{text}");
        assert!(text.contains("Health: ok\n"), "{text}");
        assert!(
            text.contains("3 sessions, 0 agents (0 busy, 0 waiting), no snapshot yet"),
            "{text}"
        );
        assert!(!text.contains("Idle for"), "{text}");
        assert!(!text.contains("setup"), "{text}");
    }

    #[test]
    fn the_sitting_is_one_line_in_words_and_never_news() {
        let b = Brief {
            sitting: "1 hr 7 min, 17 min passed break time".to_string(),
            ..Default::default()
        };
        assert!(!b.has_news());
        let text = render(&b, 1000, "/home/me");
        assert!(
            text.contains("Sitting: 1 hr 7 min, 17 min passed break time\n"),
            "{text}"
        );
        assert!(!render(&Brief::default(), 1000, "/home/me").contains("Sitting"));
    }

    #[test]
    fn the_setup_line_shows_only_with_something_open_and_is_never_news() {
        let b = Brief {
            setup_open: Some(4),
            ..Default::default()
        };
        assert!(
            !b.has_news(),
            "an open setup item must not open the popup on attach"
        );
        let text = render(&b, 1000, "/home/me");
        assert!(text.contains("4 setup items open"), "{text}");
        assert!(text.contains("tmux-companion setup"), "{text}");
        for quiet in [Some(0), None] {
            let b = Brief {
                setup_open: quiet,
                ..Default::default()
            };
            assert!(!render(&b, 1000, "/home/me").contains("setup"));
        }
    }

    #[test]
    fn a_waiting_agent_and_a_health_reason_are_news_and_come_first() {
        let pane = crate::panes::Pane {
            session: "api".into(),
            window_index: 2,
            window_name: "ai".into(),
            pane_index: 1,
            id: "%7".into(),
            command: "claude".into(),
            path: "/home/me/w/api".into(),
            title: "laptop".into(),
            visible: false,
            activity: 700,
            in_mode: false,
            host: "laptop".into(),
            bell: false,
        };
        let asked = |lines: &str| {
            crate::inbox::entry(
                &pane,
                crate::panes::State::Waiting(300),
                None,
                "/home/me",
                lines.into(),
                &[],
            )
        };
        let b = Brief {
            waiting: vec![asked("> Continue? (y/n)")],
            health: vec![
                "config.toml changed after the daemon started; run tmux-companion restart".into(),
            ],
            sessions: 1,
            agents: (1, 0, 1),
            last_snapshot: Some(("20260926T133256".into(), 720)),
            ..Default::default()
        };
        assert!(b.has_news());
        let text = render(&b, 1000, "/home/me");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "Waiting on you (1)");
        assert!(
            lines[1].contains("api:2.1")
                && lines[1].contains("5m")
                && lines[1].ends_with("> Continue? (y/n)"),
            "{}",
            lines[1]
        );
        assert!(text.contains("Health\n  config.toml changed"), "{text}");
        assert!(
            text.ends_with("1 session, 1 agent (0 busy, 1 waiting), last snapshot 12m ago\n"),
            "{text}"
        );
        // Painted, the state is amber and the question is italic, and the
        // pane it is in is neither: the tone sits on the cell, not the row.
        let painted = render_in(
            &b,
            1000,
            "/home/me",
            &crate::picker::Paint {
                escapes: true,
                ..crate::picker::Paint::default()
            },
        );
        let row = painted.lines().nth(1).unwrap_or_default();
        assert!(row.contains("\x1b[38;5;214mwaiting"), "{row:?}");
        assert!(row.contains("\x1b[3m> Continue? (y/n)"), "{row:?}");
        assert!(row.contains("\x1b[1mapi:2.1"), "{row:?}");
        assert!(!row.contains("214mapi"), "{row:?}");
        // An agent that said `done` is listed, but it is not news.
        let mut done = asked("Here is the diff.");
        done.state = "done".into();
        let quiet = Brief {
            waiting: vec![done],
            sessions: 1,
            ..Default::default()
        };
        assert!(!quiet.has_news());
        assert!(render(&quiet, 1000, "/home/me").contains("done"));
    }

    fn agent(at: &str, question: &str) -> Entry {
        let (session, rest) = at.split_once(':').unwrap_or((at, "1.0"));
        let pane = crate::panes::Pane {
            session: session.into(),
            window_index: rest[..1].parse().unwrap_or(1),
            window_name: "ai".into(),
            pane_index: rest[2..].parse().unwrap_or(0),
            id: format!("%{session}"),
            command: "claude".into(),
            path: "/home/me/w/api".into(),
            title: "laptop".into(),
            visible: false,
            activity: 700,
            in_mode: false,
            host: "laptop".into(),
            bell: false,
        };
        crate::inbox::entry(
            &pane,
            crate::panes::State::Waiting(300),
            None,
            "/home/me",
            question.into(),
            &[],
        )
    }

    fn idle(name: &str) -> IdleSession {
        IdleSession {
            name: name.into(),
            path: format!("/home/me/w/{name}"),
            windows: 1,
            idle: 5 * crate::sessions::idle::DAY,
        }
    }

    /// The mock the keys were agreed on: two agents, a health reason, one
    /// idle session.
    fn busy() -> Brief {
        Brief {
            waiting: vec![
                agent("api:2.1", "> Continue? (y/n)"),
                agent("web:1.0", "> Which branch?"),
            ],
            health: vec!["a timer failed: sessions".into()],
            idle: vec![idle("old-spike")],
            sessions: 3,
            agents: (2, 0, 2),
            last_snapshot: Some(("20260926T133256".into(), 720)),
            journal_today: 4,
            ..Default::default()
        }
    }

    #[test]
    fn numbers_run_on_from_the_agents_into_the_idle_sessions() {
        let b = busy();
        assert_eq!(
            targets(&b),
            vec![
                (Target::Pane("%api".into()), "api:2.1, claude".into()),
                (Target::Pane("%web".into()), "web:1.0, claude".into()),
                (Target::Session("old-spike".into()), "old-spike".into()),
            ]
        );
        let text = render_numbered(&b, 1000, "/home/me", &crate::picker::Paint::plain());
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[1].starts_with("  1  api:2.1"), "{}", lines[1]);
        assert!(lines[2].starts_with("  2  web:1.0"), "{}", lines[2]);
        assert!(
            text.contains("\n  3  old-spike  ~/w/old-spike  idle 5d"),
            "{text}"
        );
        // `--print` and the hook's screen carry no numbers: nothing reads
        // keys there.
        assert!(render(&b, 1000, "/home/me").contains("\n  api:2.1"));
    }

    #[test]
    fn an_agent_past_the_shown_few_has_no_number_and_the_idle_ones_follow_the_shown() {
        let b = Brief {
            waiting: vec![agent("api:2.1", "?"); SHOWN + 2],
            idle: vec![idle("old")],
            ..Default::default()
        };
        let rows = targets(&b);
        assert_eq!(rows.len(), SHOWN + 1);
        assert_eq!(rows[SHOWN].0, Target::Session("old".into()));
        assert_eq!(
            choose(&b, &format!("c {}", SHOWN + 1)),
            Action::Close("old".into())
        );
    }

    #[test]
    fn the_footer_names_only_the_keys_that_do_something_now() {
        assert_eq!(
            footer(&busy()),
            "1-3 go there   c close idle   h ack health   d doctor   i inbox   j journal   s save   z quiet 1h   q close"
        );
        let quiet = Brief {
            quiet: Some("quiet for 40m more".into()),
            ..Default::default()
        };
        assert_eq!(footer(&quiet), "d doctor   s save   z end quiet   q close");
    }

    #[test]
    fn a_number_goes_there_and_c_with_a_number_closes_an_idle_session() {
        let b = busy();
        assert_eq!(
            choose(&b, "1\n"),
            Action::Go(Target::Pane("%api".into()), "api:2.1, claude".into())
        );
        assert_eq!(
            choose(&b, " 3 "),
            Action::Go(Target::Session("old-spike".into()), "old-spike".into())
        );
        assert_eq!(choose(&b, "c3"), Action::Close("old-spike".into()));
        assert_eq!(choose(&b, "c 3"), Action::Close("old-spike".into()));
        assert_eq!(
            choose(&b, "c 1"),
            Action::Say("1 is an agent. c takes the idle session's number, 3.".into())
        );
        assert_eq!(
            choose(&b, "7"),
            Action::Say("There is no 7. The numbers go from 1 to 3.".into())
        );
        for (key, action) in [
            ("h", Action::Ack),
            ("d", Action::Doctor),
            ("i", Action::Inbox),
            ("j", Action::Journal),
            ("s", Action::Save),
            ("z", Action::Quiet),
            ("q", Action::Leave),
            ("", Action::Leave),
            ("Q", Action::Leave),
        ] {
            assert_eq!(choose(&b, key), action, "{key:?}");
        }
        assert!(
            matches!(choose(&b, "x"), Action::Say(s) if s == "x is not one of the keys; ? lists them.")
        );
    }

    #[test]
    fn a_key_with_nothing_to_act_on_says_so_and_does_nothing() {
        let b = Brief::default();
        assert!(matches!(choose(&b, "h"), Action::Say(_)));
        assert!(matches!(choose(&b, "i"), Action::Say(_)));
        assert!(matches!(choose(&b, "j"), Action::Say(_)));
        assert_eq!(
            choose(&b, "c 1"),
            Action::Say("There is no idle session to close.".into())
        );
        assert_eq!(
            choose(&b, "1"),
            Action::Say("There is no 1: nothing here has a number.".into())
        );
    }

    #[test]
    fn a_digit_answers_at_once_unless_a_second_one_could_follow() {
        assert_eq!(
            press("", Key::Char('3'), 9, true),
            Pressed::Answer("3".into())
        );
        assert_eq!(
            press("", Key::Char('1'), 12, true),
            Pressed::Pending("1".into())
        );
        assert_eq!(
            press("", Key::Char('2'), 12, true),
            Pressed::Answer("2".into())
        );
        assert_eq!(
            press("1", Key::Char('2'), 12, true),
            Pressed::Answer("12".into())
        );
        assert_eq!(
            press("1", Key::Enter, 12, true),
            Pressed::Answer("1".into())
        );
        assert_eq!(
            press("", Key::Char('0'), 12, true),
            Pressed::Answer("0".into())
        );
    }

    #[test]
    fn c_waits_for_a_number_and_escape_or_a_letter_drops_it() {
        assert_eq!(
            press("", Key::Char('c'), 4, true),
            Pressed::Pending("c".into())
        );
        assert_eq!(
            press("c", Key::Char('4'), 4, true),
            Pressed::Answer("c4".into())
        );
        assert_eq!(
            press("c", Key::Char('1'), 12, true),
            Pressed::Pending("c1".into())
        );
        assert_eq!(
            press("c", Key::Esc, 4, true),
            Pressed::Pending(String::new())
        );
        assert_eq!(
            press("c", Key::Char('q'), 4, true),
            Pressed::Pending(String::new())
        );
        assert_eq!(
            press("c1", Key::Backspace, 12, true),
            Pressed::Pending("c".into())
        );
        assert_eq!(press("", Key::Esc, 4, true), Pressed::Answer("q".into()));
        assert_eq!(
            press("c", Key::Interrupt, 4, true),
            Pressed::Answer("q".into())
        );
        assert_eq!(
            press("", Key::Char('h'), 4, true),
            Pressed::Answer("h".into())
        );
        assert_eq!(
            press("", Key::Enter, 4, true),
            Pressed::Answer(String::new())
        );
        // Nothing idle: `c` answers at once, and `choose` says why.
        assert_eq!(
            press("", Key::Char('c'), 4, false),
            Pressed::Answer("c".into())
        );
    }

    #[test]
    fn a_pending_key_says_what_it_waits_for() {
        let b = busy();
        assert_eq!(pending_line(&b, ""), None);
        assert_eq!(
            pending_line(&b, "c").as_deref(),
            Some("close which? its number, 3   esc cancels")
        );
    }

    #[test]
    fn quiet_hours_are_not_a_health_reason_on_the_brief() {
        let text = "quiet for 40m more\nconfig.toml changed after the daemon started\n\n";
        assert_eq!(
            health_reasons(text),
            ["config.toml changed after the daemon started"]
        );
        let b = Brief {
            health: health_reasons("quiet for 40m more\n"),
            quiet: Some("quiet for 40m more".into()),
            ..Default::default()
        };
        assert!(!b.has_news(), "quiet must not open the popup on attach");
        assert!(!offers(&b).contains(&Offer::Ack));
    }
}
