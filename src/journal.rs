//! What happened in each project today: the commands that ran long, the
//! questions the agents stopped on, the sessions opened and closed.
//!
//! The daemon already watches every pane for `[notify]` and the inbox; this
//! writes what it sees down, one line per event, so the standup answer is a
//! picker rather than a memory. `sessions resurrect` says what was running at
//! one moment; this says what ran between the moments.
//!
//! The file is `journal.tsv` in the state directory, unix seconds first so it
//! sorts as it happened, and it rotates past a megabyte like the daemon log.

use std::collections::HashMap;
use std::time::Instant;

use crate::notify::{self, PaneRow, Rules, Watched};

/// What kind of thing happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A command ran past `[journal] min_secs` and finished.
    Ran,
    /// An agent stopped and asked something.
    Asked,
    /// A project session was built.
    Opened,
    /// A project session was closed cleanly.
    Closed,
}

impl Kind {
    /// The word in the file and the column.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Ran => "ran",
            Kind::Asked => "asked",
            Kind::Opened => "opened",
            Kind::Closed => "closed",
        }
    }

    /// The kind a word names, when it does.
    pub fn from_word(w: &str) -> Option<Self> {
        Some(match w {
            "ran" => Kind::Ran,
            "asked" => Kind::Asked,
            "opened" => Kind::Opened,
            "closed" => Kind::Closed,
            _ => return None,
        })
    }
}

/// One line of the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// When, in unix seconds.
    pub at: u64,
    /// The session it happened in.
    pub session: String,
    /// The directory, so two sessions with one name on two machines still read.
    pub path: String,
    /// What kind.
    pub kind: Kind,
    /// The command and how long it ran, the question, or nothing.
    pub detail: String,
}

/// The line an event is written as. Tabs and newlines inside a detail are
/// folded to spaces, since a detail that broke the file would be worse than
/// one that lost its layout.
pub fn line(e: &Event) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    format!(
        "{}\t{}\t{}\t{}\t{}",
        e.at,
        clean(&e.session),
        clean(&e.path),
        e.kind.word(),
        clean(&e.detail)
    )
}

/// The event a line holds, or nothing for a line this cannot read.
pub fn parse_line(l: &str) -> Option<Event> {
    let f: Vec<&str> = l.splitn(5, '\t').collect();
    if f.len() < 4 {
        return None;
    }
    Some(Event {
        at: f[0].trim().parse().ok()?,
        session: f[1].to_string(),
        path: f[2].to_string(),
        kind: Kind::from_word(f[3].trim())?,
        detail: f.get(4).map(|s| s.to_string()).unwrap_or_default(),
    })
}

/// Every event since `since`, oldest first, for one session when named.
pub fn select<'a>(
    events: impl IntoIterator<Item = &'a Event>,
    since: u64,
    session: Option<&str>,
) -> Vec<Event> {
    events
        .into_iter()
        .filter(|e| e.at >= since)
        .filter(|e| session.is_none_or(|s| e.session == s))
        .cloned()
        .collect()
}

/// The file.
pub fn path() -> Option<std::path::PathBuf> {
    crate::server::state_dir().map(|d| d.join("journal.tsv"))
}

/// Write one event down.
///
/// A failure to write is not reported: this runs from timers and from the
/// tail of `project close`, and neither has a person to tell.
pub fn append(e: &Event) {
    use std::io::Write;
    let Some(p) = path() else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    crate::server::rotate_log(&p);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = writeln!(f, "{}", line(e));
    }
}

/// Every event on file, oldest first.
pub fn read_all() -> Vec<Event> {
    let Some(p) = path() else {
        return Vec::new();
    };
    std::fs::read_to_string(p)
        .map(|t| t.lines().filter_map(parse_line).collect())
        .unwrap_or_default()
}

/// Local clock time for a moment, `HH:MM`, and the local date as
/// `YYYY-MM-DD`; UTC would put yesterday evening under today's heading.
pub fn local(secs: u64) -> (String, String) {
    let t = libc::time_t::try_from(secs).unwrap_or(0);
    // SAFETY: `tm` is a plain C struct that `localtime_r` fills completely,
    // and both pointers are valid for the call.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    (
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
        format!(
            "{}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        ),
    )
}

/// The row for an event: the time, with the date in front when it is not
/// today's, then the session, the kind and the detail.
pub fn columns(e: &Event, today: &str) -> Vec<String> {
    let (hm, ymd) = local(e.at);
    let when = if ymd == today {
        hm
    } else {
        format!("{ymd} {hm}")
    };
    vec![
        when,
        e.session.clone(),
        e.kind.word().to_string(),
        e.detail.clone(),
    ]
}

/// The daemon's task: watch every pane and write down what finishes.
///
/// Same tracker as `[notify]`, run with its own threshold and no interest in
/// whether anybody was watching: the point is the record, not the interruption.
/// Programs in `[notify] ignore` are not runs worth writing down either.
pub async fn journal_loop(
    config: crate::config::Journal,
    ignore: Vec<String>,
    shell: String,
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
) {
    let interval = std::time::Duration::from_secs(config.interval_secs.max(1));
    let threshold = std::time::Duration::from_secs(config.min_secs);
    let mut watched: HashMap<String, Watched> = HashMap::new();
    let home = std::env::var("HOME").unwrap_or_default();
    loop {
        tokio::time::sleep(interval).await;
        let panes = crate::panes::list().await;
        let rows: Vec<PaneRow> = panes
            .iter()
            .map(|p| PaneRow {
                id: p.id.clone(),
                command: p.command.clone(),
                visible: p.visible,
            })
            .collect();
        let rules = Rules {
            shell: &shell,
            ignore: &ignore,
            threshold,
            only_when_unwatched: false,
        };
        let finished = notify::step(&mut watched, &rows, &rules, Instant::now());
        if finished.is_empty() {
            continue;
        }
        // Read once, under the lock, so a burst of finishes is one lock.
        let _ = state.lock().await;
        let now = crate::panes::now_secs();
        for f in finished {
            let Some(pane) = panes.iter().find(|p| p.id == f.pane) else {
                continue;
            };
            append(&Event {
                at: now,
                session: pane.session.clone(),
                path: crate::project::short_path(&pane.path, &home),
                kind: Kind::Ran,
                detail: format!("{} {}", f.command, notify::human(f.ran_for)),
            });
        }
    }
}

/// `journal`: the events, newest first, and a jump to the session picked.
pub async fn run(print: bool, project: Option<String>, days: u64) -> anyhow::Result<()> {
    let config = crate::cli::config_or_default();
    let now = crate::panes::now_secs();
    let since = now.saturating_sub(days.max(1).saturating_mul(86_400));
    let all = read_all();
    let mut events = select(&all, since, project.as_deref());
    events.reverse();
    if events.is_empty() {
        eprintln!(
            "nothing in the journal for the last {} day{}{}",
            days.max(1),
            if days.max(1) == 1 { "" } else { "s" },
            project
                .as_deref()
                .map(|p| format!(" in {p}"))
                .unwrap_or_default()
        );
        return Ok(());
    }
    let (_, today) = local(now);
    if print {
        for e in &events {
            println!("{}", columns(e, &today).join("\t"));
        }
        return Ok(());
    }
    let items: Vec<crate::picker::Item> = events
        .iter()
        .map(|e| {
            let cols = columns(e, &today);
            crate::picker::Item::new(cols.join(" ")).in_columns(cols)
        })
        .collect();
    let chrome = crate::picker::Chrome {
        title: "[ Journal ]".into(),
        footer: "enter goes to that session   ctrl-a clears the filter   esc closes".into(),
        preview_title: String::new(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Project);
    if let Some(index) = crate::picker::run(items, "", &chrome)?
        && let Some(e) = events.get(index)
    {
        let target = format!("={}", e.session);
        crate::cli::tmux(&["switch-client", "-t", &target]).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: u64, session: &str, kind: Kind, detail: &str) -> Event {
        Event {
            at,
            session: session.into(),
            path: "~/w/api".into(),
            kind,
            detail: detail.into(),
        }
    }

    #[test]
    fn a_line_reads_back_as_the_event_it_was_written_from() {
        let e = event(1_790_000_000, "api", Kind::Ran, "cargo test 2m 05s");
        assert_eq!(parse_line(&line(&e)), Some(e.clone()));
        let asked = event(1_790_000_001, "api", Kind::Asked, "Continue?\twith\ntabs");
        let back = parse_line(&line(&asked)).unwrap();
        assert_eq!(
            back.detail, "Continue? with tabs",
            "the detail is folded to one line"
        );
        assert_eq!(parse_line("garbage"), None);
        assert_eq!(
            parse_line("1\tapi\t~/w\twondered\t"),
            None,
            "an unknown kind is skipped"
        );
    }

    #[test]
    fn selection_is_by_time_and_then_by_session() {
        let all = vec![
            event(100, "api", Kind::Opened, ""),
            event(200, "web", Kind::Ran, "make 1m 00s"),
            event(300, "api", Kind::Closed, ""),
        ];
        assert_eq!(select(&all, 150, None).len(), 2);
        assert_eq!(select(&all, 0, Some("api")).len(), 2);
        assert_eq!(select(&all, 250, Some("api")).len(), 1);
    }

    #[test]
    fn the_clock_reads_as_a_time_and_a_date() {
        let (hm, ymd) = local(1_790_000_000);
        assert_eq!(hm.len(), 5);
        assert_eq!(&hm[2..3], ":");
        assert_eq!(ymd.len(), 10);
        assert!(ymd.starts_with("2026-"), "{ymd}");
    }

    #[test]
    fn the_date_shows_only_when_it_is_not_today() {
        let e = event(1_790_000_000, "api", Kind::Ran, "x");
        let (_, its_day) = local(e.at);
        assert_eq!(columns(&e, &its_day)[0].len(), 5);
        assert!(columns(&e, "1999-01-01")[0].starts_with("2026-"));
    }

    #[test]
    fn the_words_round_trip() {
        for k in [Kind::Ran, Kind::Asked, Kind::Opened, Kind::Closed] {
            assert_eq!(Kind::from_word(k.word()), Some(k));
        }
    }
}
