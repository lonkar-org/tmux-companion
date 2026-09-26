//! The agents waiting on you, with the question each one asked.
//!
//! The bar's `agents` segment says how many have stopped; this says which,
//! and shows the last lines of each one's screen, which is where the question
//! is. The daemon keeps it: every `[agents] interval_secs` it reads the pane
//! list, and the moment an agent flips from busy to waiting it captures that
//! pane's tail, so the question is on record even if the agent is on a window
//! nobody has looked at since. An agent that draws again drops off the list.
//!
//! The capture happens once per flip rather than on every read, so an agent
//! that waits an hour costs one `capture-pane`, not eighteen hundred.
//!
//! An agent that reports through its hooks (see `agent.rs`) arrives here as
//! `asked` or `done` the moment it says so, with the wait counted from the
//! hook rather than from the window's last draw; one that says nothing
//! arrives as `waiting` when the window has been quiet long enough, or as
//! `asked` when it rang the bell.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::panes::{self, Pane, Reported, Reports, State};

/// One agent that has stopped, as the daemon last saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// `#{pane_id}`.
    pub id: String,
    /// `session:window.pane`, for the row.
    pub at: String,
    /// The program, with the pane's title when one was set.
    pub program: String,
    /// The directory it is in.
    pub path: String,
    /// When it stopped, in unix seconds: when the hook said so, or the
    /// window's activity time at the flip, which is when it last drew.
    pub since: u64,
    /// The last lines of its screen at the flip.
    pub lines: String,
    /// How it stopped: `asked`, `done` or `waiting`, the state's first word.
    #[serde(default = "waiting_word")]
    pub state: String,
}

/// The word an entry written before the field existed reads back with.
fn waiting_word() -> String {
    "waiting".to_string()
}

/// What one read of the pane list changes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Panes that were busy, or new, and are now waiting, with how: capture
    /// these.
    pub arrived: Vec<(Pane, State)>,
    /// Entries that are still waiting, keyed by pane id.
    pub kept: HashMap<String, Entry>,
}

/// Compare a fresh listing with what the inbox holds.
///
/// An entry survives while its pane is still an agent that is still waiting.
/// A pane that drew again, went into copy mode, changed program or vanished
/// takes its entry with it, and the next time it stops it arrives fresh, with
/// a new capture, because the question will be a new one.
pub fn step(
    held: &HashMap<String, Entry>,
    panes: &[Pane],
    programs: &[String],
    reports: &Reports,
    now: u64,
    waiting_secs: u64,
) -> Step {
    let mut out = Step::default();
    for pane in panes {
        if !panes::is_agent(&pane.command, programs) {
            continue;
        }
        let report = reports.get(&pane.id).copied();
        let state = panes::state(pane, true, report, now, waiting_secs);
        if !state.is_waiting() {
            continue;
        }
        let since = stopped_at(pane, report);
        match held.get(&pane.id) {
            Some(entry) if entry.since == since && entry.state == state.word() => {
                out.kept.insert(pane.id.clone(), entry.clone());
            }
            // Either new, or it drew something since the capture and stopped
            // again, or a hook has since said something else: the question
            // on screen may not be the one on record.
            _ => out.arrived.push((pane.clone(), state)),
        }
    }
    out
}

/// When a stopped agent stopped: the hook's time when it reported, the
/// window's last draw otherwise.
fn stopped_at(pane: &Pane, report: Option<Reported>) -> u64 {
    match report {
        Some(r) if r.state != panes::Report::Busy => r.at,
        _ => pane.activity,
    }
}

/// An entry for a pane that just stopped, with what its screen held.
pub fn entry(
    pane: &Pane,
    state: State,
    report: Option<Reported>,
    home: &str,
    lines: String,
) -> Entry {
    Entry {
        id: pane.id.clone(),
        at: format!("{}:{}.{}", pane.session, pane.window_index, pane.pane_index),
        program: panes::program(pane),
        path: crate::project::short_path(&pane.path, home),
        since: stopped_at(pane, report),
        lines,
        state: state.word().to_string(),
    }
}

/// The question, as one line: the last non-empty line of the capture, which
/// is where a prompt waiting for an answer sits.
pub fn question(lines: &str) -> String {
    lines
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Which entries have waited past `after` seconds and have not been nudged.
///
/// `after` of zero means never, which is the default: a nudge is a
/// notification, and the bar already says how many are waiting.
pub fn due(
    entries: &HashMap<String, Entry>,
    nudged: &HashSet<String>,
    now: u64,
    after: u64,
) -> Vec<Entry> {
    if after == 0 {
        return Vec::new();
    }
    let mut out: Vec<Entry> = entries
        .values()
        .filter(|e| !nudged.contains(&e.id) && now.saturating_sub(e.since) >= after)
        .cloned()
        .collect();
    out.sort_by(|a, b| a.at.cmp(&b.at));
    out
}

/// The command that nudges about one entry.
///
/// Empty means tmux's own `display-message`. A configured one gets
/// `{program}`, `{at}`, `{waited}` and `{question}` substituted anywhere they
/// appear, so a desktop notifier can be given a title and a body.
pub fn nudge_command(e: &Entry, now: u64, configured: &[String]) -> Vec<String> {
    let waited = panes::age(now.saturating_sub(e.since));
    let question = question(&e.lines);
    if configured.is_empty() {
        return vec![
            "tmux".to_string(),
            "display-message".to_string(),
            "-d".to_string(),
            "4000".to_string(),
            format!(
                "tmux-companion: {} in {} has waited {waited}",
                e.program, e.at
            ),
        ];
    }
    configured
        .iter()
        .map(|a| {
            a.replace("{program}", &e.program)
                .replace("{at}", &e.at)
                .replace("{waited}", &waited)
                .replace("{question}", &question)
        })
        .collect()
}

/// Entries in row order: the longest wait first, since that is the one most
/// overdue an answer.
pub fn ordered(entries: &HashMap<String, Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = entries.values().cloned().collect();
    out.sort_by(|a, b| a.since.cmp(&b.since).then(a.at.cmp(&b.at)));
    out
}

/// The daemon's task: read, compare, capture the arrivals, nudge the overdue.
///
/// The pane list is read outside the lock and the captures run outside it
/// too; the lock is taken once to swap the inbox and once more to note what
/// was nudged. The agents segment's sample is stored on the way past, so the
/// bar reads a warm cache while this runs.
pub async fn inbox_loop(
    config: crate::config::Agents,
    journal: bool,
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
) {
    let interval = std::time::Duration::from_secs(config.interval_secs.max(1));
    let home = std::env::var("HOME").unwrap_or_default();
    loop {
        tokio::time::sleep(interval).await;
        let panes = panes::list().await;
        let now = panes::now_secs();
        let (held, reports) = {
            let mut st = state.lock().await;
            st.retain_reports(&panes);
            (st.inbox.clone(), st.reports.clone())
        };
        let Step { arrived, mut kept } = step(
            &held,
            &panes,
            &config.programs,
            &reports,
            now,
            config.waiting_secs,
        );
        for (pane, how) in &arrived {
            let lines = panes::tail_of(&pane.id).await;
            let e = entry(pane, *how, reports.get(&pane.id).copied(), &home, lines);
            // An agent that said `done` answered rather than asked, so the
            // journal, which records questions, is left alone.
            if journal && !matches!(how, State::Done(_)) {
                crate::journal::append(&crate::journal::Event {
                    at: now,
                    session: pane.session.clone(),
                    path: e.path.clone(),
                    kind: crate::journal::Kind::Asked,
                    detail: format!("{} {}", e.program, question(&e.lines)),
                });
            }
            kept.insert(pane.id.clone(), e);
        }
        let sample = crate::segments::agents::count(
            &panes,
            &config.programs,
            &reports,
            now,
            config.waiting_secs,
        );
        let to_nudge = {
            let mut st = state.lock().await;
            st.agents_store(sample);
            st.nudged.retain(|id| kept.contains_key(id));
            st.inbox = kept;
            // Quiet hours hold the nudge; the entry stays, so it fires when
            // quiet ends if the agent is still waiting.
            if st.is_quiet() {
                Vec::new()
            } else {
                due(&st.inbox, &st.nudged, now, config.nudge_after_secs)
            }
        };
        for e in to_nudge {
            let cmd = nudge_command(&e, now, &config.nudge_command);
            if let Some((program, args)) = cmd.split_first() {
                let _ = tokio::process::Command::new(program)
                    .args(args)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await;
            }
            state.lock().await.nudged.insert(e.id);
        }
    }
}

/// `inbox`: the waiting agents and their questions, and a jump to the pick.
pub async fn run(print: bool) -> anyhow::Result<()> {
    let config = crate::cli::config_or_default();
    let resp = crate::client::send(crate::proto::Request::raw(
        "__inbox",
        serde_json::Value::Null,
    ))
    .await?;
    if let Some(why) = resp.error {
        anyhow::bail!("{why}");
    }
    let entries: Vec<Entry> = serde_json::from_str(&resp.output).unwrap_or_default();
    if entries.is_empty() {
        eprintln!("no agent is waiting on you");
        return Ok(());
    }
    let now = panes::now_secs();
    if print {
        for e in &entries {
            println!(
                "{}\t{}\t{} {}\t{}\t{}",
                e.at,
                e.program,
                e.state,
                panes::age(now.saturating_sub(e.since)),
                question(&e.lines),
                e.id
            );
        }
        return Ok(());
    }
    let items: Vec<crate::picker::Item> = entries
        .iter()
        .map(|e| {
            let waited = format!("{} {}", e.state, panes::age(now.saturating_sub(e.since)));
            let q = question(&e.lines);
            crate::picker::Item::with_preview(
                format!("{} {} {} {}", e.at, e.program, waited, q),
                e.lines.clone(),
            )
            .in_columns(vec![e.at.clone(), e.program.clone(), waited, q])
            // A `done` agent answered: it is on the list so you can read
            // the answer, not because it needs one, so it is not coloured.
            .in_colour((e.state != "done").then(|| panes::WAITING_COLOUR.to_string()))
        })
        .collect();
    let chrome = crate::picker::Chrome {
        title: "[ Inbox ]".into(),
        footer: "enter jumps there   esc leaves them waiting".into(),
        preview_title: "[ Where it stopped ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Panes);
    if let Some(index) = crate::picker::run(items, "", &chrome)?
        && let Some(e) = entries.get(index)
    {
        panes::jump(&e.id).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, command: &str, activity: u64, in_mode: bool) -> Pane {
        Pane {
            session: "api".into(),
            window_index: 2,
            window_name: "ai".into(),
            pane_index: 1,
            id: id.into(),
            command: command.into(),
            path: "/home/me/w/api".into(),
            title: "laptop".into(),
            visible: true,
            activity,
            in_mode,
            host: "laptop".into(),
            bell: false,
        }
    }

    fn programs() -> Vec<String> {
        vec!["claude".to_string()]
    }

    fn quiet(pane: &Pane, lines: &str) -> Entry {
        entry(pane, State::Waiting(0), None, "/home/me", lines.into())
    }

    fn said(id: &str, state: panes::Report, at: u64) -> Reports {
        Reports::from([(id.to_string(), Reported { state, at })])
    }

    #[test]
    fn a_busy_agent_is_not_in_the_inbox_and_a_quiet_one_arrives() {
        let p = programs();
        let held = HashMap::new();
        let none = Reports::new();
        let busy = step(
            &held,
            &[pane("%1", "claude", 1000, false)],
            &p,
            &none,
            1005,
            10,
        );
        assert!(busy.arrived.is_empty() && busy.kept.is_empty());
        let quiet = step(
            &held,
            &[pane("%1", "claude", 1000, false)],
            &p,
            &none,
            1020,
            10,
        );
        assert_eq!(quiet.arrived.len(), 1);
        assert_eq!(quiet.arrived[0].1, State::Waiting(20));
    }

    #[test]
    fn a_report_arrives_at_once_and_is_dated_by_the_hook() {
        let p = programs();
        let held = HashMap::new();
        // Drew a second ago, but said asked: in the inbox now, not in ten.
        let asked = said("%1", panes::Report::Asked, 1004);
        let now = step(
            &held,
            &[pane("%1", "claude", 1004, false)],
            &p,
            &asked,
            1005,
            10,
        );
        assert_eq!(now.arrived.len(), 1);
        assert_eq!(now.arrived[0].1, State::Asked(1));
        let e = entry(
            &now.arrived[0].0,
            now.arrived[0].1,
            asked.get("%1").copied(),
            "/h",
            "q".into(),
        );
        assert_eq!(e.since, 1004, "the hook's time, not the window's");
        assert_eq!(e.state, "asked");
        // Quiet for a minute, but said busy: a long tool, not a stop.
        let busy = said("%1", panes::Report::Busy, 940);
        let none = step(
            &held,
            &[pane("%1", "claude", 940, false)],
            &p,
            &busy,
            1005,
            10,
        );
        assert!(none.arrived.is_empty());
        // Said done: on the list as done, so the answer can be read.
        let done = said("%1", panes::Report::Done, 1000);
        let out = step(
            &held,
            &[pane("%1", "claude", 1000, false)],
            &p,
            &done,
            1005,
            10,
        );
        assert_eq!(out.arrived[0].1, State::Done(5));
    }

    #[test]
    fn an_entry_is_kept_while_it_waits_and_recaptured_after_it_draws_again() {
        let p = programs();
        let none = Reports::new();
        let e = quiet(&pane("%1", "claude", 1000, false), "Continue? (y/n)");
        let held = HashMap::from([("%1".to_string(), e.clone())]);
        // Same activity: still the same stop, kept as is.
        let same = step(
            &held,
            &[pane("%1", "claude", 1000, false)],
            &p,
            &none,
            1100,
            10,
        );
        assert_eq!(same.kept.get("%1"), Some(&e));
        assert!(same.arrived.is_empty());
        // It drew at 1050 and stopped again: a new question, so it arrives.
        let again = step(
            &held,
            &[pane("%1", "claude", 1050, false)],
            &p,
            &none,
            1100,
            10,
        );
        assert!(again.kept.is_empty());
        assert_eq!(again.arrived.len(), 1);
        // A hook saying `done` about the same quiet pane is a new stop too.
        let done = said("%1", panes::Report::Done, 1000);
        let spoke = step(
            &held,
            &[pane("%1", "claude", 1000, false)],
            &p,
            &done,
            1100,
            10,
        );
        assert!(spoke.kept.is_empty() && spoke.arrived.len() == 1);
        // Reading it, or a shell in its place, drops it.
        assert!(
            step(
                &held,
                &[pane("%1", "claude", 1000, true)],
                &p,
                &none,
                1100,
                10
            )
            .kept
            .is_empty()
        );
        assert!(
            step(
                &held,
                &[pane("%1", "zsh", 1000, false)],
                &p,
                &none,
                1100,
                10
            )
            .kept
            .is_empty()
        );
        assert!(step(&held, &[], &p, &none, 1100, 10).kept.is_empty());
    }

    #[test]
    fn an_entry_written_before_the_state_field_reads_back_as_waiting() {
        let json =
            r#"{"id":"%1","at":"api:2.1","program":"claude","path":"~/w","since":1,"lines":"q"}"#;
        let e: Entry = serde_json::from_str(json).unwrap();
        assert_eq!(e.state, "waiting");
    }

    #[test]
    fn the_question_is_the_last_line_with_anything_on_it() {
        assert_eq!(
            question("some output\n\n> Continue? (y/n)\n\n  \n"),
            "> Continue? (y/n)"
        );
        assert_eq!(question("\n\n"), "");
    }

    #[test]
    fn nudges_are_due_once_past_the_threshold_and_never_when_it_is_zero() {
        let e = quiet(&pane("%1", "claude", 1000, false), "q");
        let entries = HashMap::from([("%1".to_string(), e)]);
        let none = HashSet::new();
        assert!(due(&entries, &none, 1200, 0).is_empty());
        assert!(due(&entries, &none, 1200, 300).is_empty());
        assert_eq!(due(&entries, &none, 1300, 300).len(), 1);
        let done = HashSet::from(["%1".to_string()]);
        assert!(due(&entries, &done, 1300, 300).is_empty());
    }

    #[test]
    fn the_nudge_says_who_where_and_how_long_and_takes_a_configured_shape() {
        let e = quiet(&pane("%1", "claude", 1000, false), "> Continue?");
        let plain = nudge_command(&e, 1300, &[]);
        assert_eq!(plain[0], "tmux");
        assert!(
            plain[4].contains("claude in api:2.1 has waited 5m"),
            "{plain:?}"
        );
        let custom = nudge_command(
            &e,
            1300,
            &[
                "notify".to_string(),
                "{program} at {at}".to_string(),
                "{question} ({waited})".to_string(),
            ],
        );
        assert_eq!(custom, ["notify", "claude at api:2.1", "> Continue? (5m)"]);
    }

    #[test]
    fn rows_come_longest_wait_first() {
        let a = quiet(&pane("%1", "claude", 1000, false), "");
        let b = quiet(&pane("%2", "claude", 900, false), "");
        let entries = HashMap::from([("%1".to_string(), a), ("%2".to_string(), b)]);
        let rows = ordered(&entries);
        assert_eq!(rows[0].id, "%2");
    }
}
