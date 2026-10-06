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
    /// The one line of `lines` that is the question, picked at the capture
    /// with `[agents] question_skip`. Empty from a daemon that did not pick
    /// one, and then [`Entry::question_line`] picks with the shipped list.
    #[serde(default)]
    pub question: String,
}

impl Entry {
    /// The question, as one line.
    pub fn question_line(&self) -> String {
        if self.question.is_empty() {
            question(&self.lines)
        } else {
            self.question.clone()
        }
    }
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
    skip: &[regex::Regex],
) -> Entry {
    Entry {
        id: pane.id.clone(),
        at: format!("{}:{}.{}", pane.session, pane.window_index, pane.pane_index),
        program: panes::program(pane),
        path: crate::project::short_path(&pane.path, home),
        since: stopped_at(pane, report),
        question: question_in(&lines, skip),
        lines,
        state: state.word().to_string(),
    }
}

/// `[agents] question_skip` compiled. A pattern that does not parse is left
/// out, the bargain every other pattern in the config makes: one bad row
/// costs its own line and not the inbox.
pub fn compile(patterns: &[String]) -> Vec<regex::Regex> {
    patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
}

/// Whether a line is a horizontal rule: ten or more rule characters and
/// nothing else, which is what an agent draws above and below its input box.
fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.chars().count() >= 10 && t.chars().all(|c| matches!(c, '─' | '━' | '═' | '-'))
}

/// The screen above the input box.
///
/// claude, and the agents drawn like it, end the screen with a rule, a
/// prompt, a rule, and then a status line and a mode line of their own. None
/// of that is anything the agent said, and the status line is whatever its
/// owner configured, so no list of patterns could name it. The last rule with
/// a prompt straight under it is where the box begins, and everything from
/// there down is cut.
fn above_the_input_box<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let is_prompt = |l: &str| {
        let t = l.trim_start();
        t.starts_with('❯') || t.starts_with('>')
    };
    let cut = (0..lines.len())
        .rev()
        .find(|&i| is_rule(lines[i]) && lines.get(i + 1).is_some_and(|n| is_prompt(n)));
    match cut {
        Some(i) => lines[..i].to_vec(),
        None => lines.to_vec(),
    }
}

/// The question, as one line: the last line of the capture that is something
/// the agent said, which is where a prompt waiting for an answer sits.
///
/// The input box and what is under it go first, then every line a `skip`
/// pattern matches, and the edges of a box are taken off what is left, so a
/// question inside a dialog reads as the question. When that leaves nothing,
/// the last line with anything on it stands in, since a line of furniture
/// says more than an empty row.
pub fn question_in(lines: &str, skip: &[regex::Regex]) -> String {
    let all: Vec<&str> = lines.lines().filter(|l| !l.trim().is_empty()).collect();
    let said = above_the_input_box(&all);
    let unboxed = |l: &str| {
        l.trim()
            .trim_matches(|c| matches!(c, '│' | '┃' | '|'))
            .trim()
            .to_string()
    };
    said.iter()
        .rev()
        .map(|l| unboxed(l))
        .find(|l| !l.is_empty() && !skip.iter().any(|re| re.is_match(l)))
        .or_else(|| all.last().map(|l| l.trim().to_string()))
        .unwrap_or_default()
}

/// [`question_in`] with the shipped patterns, for a capture whose question
/// was not picked when it was taken.
pub fn question(lines: &str) -> String {
    let shipped: Vec<String> = crate::config::QUESTION_SKIP
        .iter()
        .map(|s| s.to_string())
        .collect();
    question_in(lines, &compile(&shipped))
}

/// Whether this pane's question is the one last written to the journal for
/// it, and if it is not, remember it as the last.
///
/// An agent with no hooks that redraws its status line and goes quiet again
/// is a new stop every time as far as the window can tell, and wrote the same
/// line every thirty seconds. Nothing new was said, so nothing new is
/// written.
pub fn is_repeat(last: &mut HashMap<String, String>, id: &str, question: &str) -> bool {
    if last.get(id).is_some_and(|q| q == question) {
        return true;
    }
    last.insert(id.to_string(), question.to_string());
    false
}

/// Which entries have waited past `after` seconds and have not been nudged.
///
/// `after` of zero means never, which is the default: a nudge is a
/// notification, and the bar already says how many are waiting. An agent
/// that said `done` is never due: it answered, and "has waited five
/// minutes" about every finished reply is the notification people turn off.
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
        .filter(|e| e.state != "done")
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
///
/// `message_ms` is how long tmux shows it, zero until a key is pressed:
/// `[notify] message_ms`, one setting for every message this daemon puts up.
pub fn nudge_command(e: &Entry, now: u64, configured: &[String], message_ms: u64) -> Vec<String> {
    let waited = panes::age(now.saturating_sub(e.since));
    let question = e.question_line();
    if configured.is_empty() {
        return vec![
            "tmux".to_string(),
            "display-message".to_string(),
            "-d".to_string(),
            message_ms.to_string(),
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

/// When each agent's current turn began, keyed by pane id: the first tick it
/// read as busy. A `done` closes the turn, and one that ran past
/// `[journal] agent_min_secs` is worth a journal line.
///
/// A question in the middle does not close it: the wait for the answer is
/// part of how long the task took. A pane that vanishes, or goes quiet with
/// nothing reported, is forgotten, so an agent without hooks never gets a
/// line here.
pub fn turns_step(turns: &mut HashMap<String, u64>, seen: &[(String, State)], now: u64) {
    let mut live = HashSet::new();
    for (id, state) in seen {
        live.insert(id.as_str());
        match state {
            State::Busy => {
                turns.entry(id.clone()).or_insert(now);
            }
            State::Asked(_) | State::Done(_) | State::Reading => {}
            _ => {
                turns.remove(id);
            }
        }
    }
    turns.retain(|id, _| live.contains(id.as_str()));
}

/// The daemon's task: read, compare, capture the arrivals, nudge the overdue.
///
/// The pane list is read outside the lock and the captures run outside it
/// too; the lock is taken once to swap the inbox and once more to note what
/// was nudged. The agents segment's sample is stored on the way past, so the
/// bar reads a warm cache while this runs.
pub async fn inbox_loop(
    config: crate::config::Agents,
    journal: crate::config::Journal,
    message_ms: u64,
    earcons: crate::config::Earcons,
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
) {
    let interval = std::time::Duration::from_secs(config.interval_secs.max(1));
    let home = std::env::var("HOME").unwrap_or_default();
    let mut turns: HashMap<String, u64> = HashMap::new();
    let mut last_asked: HashMap<String, String> = HashMap::new();
    let skip = compile(&config.question_skip);
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
        let seen: Vec<(String, State)> = panes
            .iter()
            .filter(|p| panes::is_agent(&p.command, &config.programs))
            .map(|p| {
                let s = panes::state(
                    p,
                    true,
                    reports.get(&p.id).copied(),
                    now,
                    config.waiting_secs,
                );
                (p.id.clone(), s)
            })
            .collect();
        turns_step(&mut turns, &seen, now);
        // A pane that has gone, or an agent that said it is working again,
        // may ask the same thing twice and mean it.
        last_asked.retain(|id, _| {
            seen.iter().any(|(s, _)| s == id)
                && reports
                    .get(id)
                    .is_none_or(|r| r.state != panes::Report::Busy)
        });
        // One sound per pass, not one per agent: three arriving at once are
        // one thing to go and look at, and three tones on top of each other
        // are noise.
        // An agent that stopped, for whatever reason, ended a turn: a
        // boundary the chunk clock's cue may wait for.
        if !arrived.is_empty() {
            state.lock().await.chunk_boundary_at = crate::panes::now_secs();
        }
        if !arrived.is_empty() && earcons.enabled {
            let quiet = state.lock().await.is_quiet();
            if arrived
                .iter()
                .any(|(_, how)| !matches!(how, State::Done(_)))
            {
                crate::earcons::sound(&earcons, crate::config::EarconEvent::Asked, quiet);
            } else {
                crate::earcons::sound(&earcons, crate::config::EarconEvent::Done, quiet);
            }
        }
        for (pane, how) in &arrived {
            let lines = panes::tail_of(&pane.id).await;
            let e = entry(
                pane,
                *how,
                reports.get(&pane.id).copied(),
                &home,
                lines,
                &skip,
            );
            if journal.enabled {
                let event = |kind, detail| crate::journal::Event {
                    at: now,
                    session: pane.session.clone(),
                    path: e.path.clone(),
                    kind,
                    detail,
                };
                match how {
                    // An agent that said `done` answered rather than asked:
                    // a line when the turn ran long enough to be work.
                    State::Done(_) => {
                        if let Some(began) = turns.remove(&pane.id)
                            && now.saturating_sub(began) >= journal.agent_min_secs
                        {
                            crate::journal::append(&event(
                                crate::journal::Kind::Answered,
                                format!(
                                    "{} {} {}",
                                    e.program,
                                    panes::age(now.saturating_sub(began)),
                                    e.question
                                ),
                            ));
                        }
                    }
                    _ => {
                        if !is_repeat(&mut last_asked, &pane.id, &e.question) {
                            crate::journal::append(&event(
                                crate::journal::Kind::Asked,
                                format!("{} {}", e.program, e.question),
                            ));
                        }
                    }
                }
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
            let cmd = nudge_command(&e, now, &config.nudge_command, message_ms);
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
        crate::picker::say_nothing_to_show("no agent is waiting on you", print).await;
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
                e.question_line(),
                e.id
            );
        }
        return Ok(());
    }
    let items: Vec<crate::picker::Item> = entries
        .iter()
        .map(|e| {
            use crate::picker::{Cell, Tone};
            use crate::tmux::icons;
            let age = panes::age(now.saturating_sub(e.since));
            let q = e.question_line();
            // A `done` agent answered: it is on the list so you can read
            // the answer, not because it needs one, so it is not amber.
            let (icon, tone) = match e.state.as_str() {
                "done" => (icons::CHECK, Tone::Dim),
                "asked" => (icons::WAITING, Tone::Asked),
                _ => (icons::WAITING, Tone::Waiting),
            };
            crate::picker::Item::with_preview(
                format!("{} {} {} {} {}", e.at, e.program, e.state, age, q),
                e.lines.clone(),
            )
            .in_cells(vec![
                Cell::strong(&e.at),
                Cell::plain(&e.program),
                Cell::new(&e.state, tone),
                Cell::dim(age),
                // The agent's words, not ours.
                Cell::quote(q),
            ])
            .with_icon(icon, tone)
        })
        .collect();
    let chrome = crate::picker::Chrome {
        title: "[ Inbox ]".into(),
        icon: crate::tmux::icons::WAITING.into(),
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
        entry(
            pane,
            State::Waiting(0),
            None,
            "/home/me",
            lines.into(),
            &shipped(),
        )
    }

    fn shipped() -> Vec<regex::Regex> {
        compile(&crate::config::Agents::default().question_skip)
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
            &shipped(),
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
    fn a_turn_runs_from_the_first_busy_tick_and_survives_a_question() {
        let mut turns = HashMap::new();
        let busy = |id: &str| (id.to_string(), State::Busy);
        turns_step(&mut turns, &[busy("%1")], 1000);
        turns_step(&mut turns, &[busy("%1")], 1010);
        assert_eq!(turns.get("%1"), Some(&1000), "the first tick, not the last");
        // A question in the middle keeps the clock running.
        turns_step(&mut turns, &[("%1".into(), State::Asked(3))], 1020);
        assert_eq!(turns.get("%1"), Some(&1000));
        // Somebody reading it does too.
        turns_step(&mut turns, &[("%1".into(), State::Reading)], 1030);
        assert_eq!(turns.get("%1"), Some(&1000));
        // Silence with nothing reported forgets it, and so does the pane going.
        turns_step(&mut turns, &[("%1".into(), State::Waiting(20))], 1040);
        assert!(turns.is_empty());
        turns_step(&mut turns, &[busy("%1")], 1050);
        turns_step(&mut turns, &[], 1060);
        assert!(turns.is_empty());
    }

    #[test]
    fn an_entry_written_before_the_state_field_reads_back_as_waiting() {
        let json =
            r#"{"id":"%1","at":"api:2.1","program":"claude","path":"~/w","since":1,"lines":"q"}"#;
        let e: Entry = serde_json::from_str(json).unwrap();
        assert_eq!(e.state, "waiting");
    }

    #[test]
    fn every_shipped_pattern_parses() {
        let patterns = crate::config::Agents::default().question_skip;
        assert_eq!(shipped().len(), patterns.len());
        // And one that does not is left out rather than taking the rest down.
        assert_eq!(compile(&["(".to_string(), "^x".to_string()]).len(), 1);
    }

    #[test]
    fn the_question_is_what_was_said_above_the_input_box() {
        // A finished claude turn, as the pane draws it.
        let done = "\
\u{23fa} The build passes. Want me to push it?
\u{273b} Baked for 37s
\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
\u{276f}\u{a0}
\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
  Fable 5.1 | [\u{2588}\u{2591}] 36% ctx | 17% 5h
  \u{23f5}\u{23f5} auto mode on (shift+tab to cycle) \u{b7} \u{2190} 1 agent
";
        assert_eq!(
            question_in(done, &shipped()),
            "\u{23fa} The build passes. Want me to push it?"
        );
        // Working: the spinner and the tool output are not the question.
        let working = "\
\u{23fa} Running the tests
  \u{23bf}  $ cargo test
\u{2722} Meandering\u{2026} (51s \u{b7} \u{2193} 3.2k tokens)
\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
\u{276f} 
\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
";
        assert_eq!(
            question_in(working, &shipped()),
            "\u{23fa} Running the tests"
        );
        // A permission dialog: the question, not its options or its hint.
        let dialog = "\
\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}
\u{2502} Bash command
\u{2502}   rm -rf target
\u{2502} Do you want to proceed?
\u{2502} \u{276f} 1. Yes
\u{2502}   2. No, and tell Claude what to do differently
\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}
  Esc to cancel
";
        assert_eq!(question_in(dialog, &shipped()), "Do you want to proceed?");
        // Nothing but furniture: the last line stands in for an empty row.
        let bare = "\u{23f5}\u{23f5} auto mode on\n";
        assert_eq!(
            question_in(bare, &shipped()),
            "\u{23f5}\u{23f5} auto mode on"
        );
        // With no patterns only the input box is cut.
        assert_eq!(question_in(done, &[]), "\u{273b} Baked for 37s");
    }

    #[test]
    fn a_done_agent_is_never_nudged_about() {
        let mut e = quiet(&pane("%1", "claude", 1000, false), "here is the diff");
        e.state = "done".into();
        let entries = HashMap::from([("%1".to_string(), e.clone())]);
        assert!(due(&entries, &HashSet::new(), 9_000, 300).is_empty());
        e.state = "asked".into();
        let entries = HashMap::from([("%1".to_string(), e)]);
        assert_eq!(due(&entries, &HashSet::new(), 9_000, 300).len(), 1);
    }

    #[test]
    fn the_same_question_from_the_same_pane_is_written_once() {
        let mut last = HashMap::new();
        assert!(!is_repeat(&mut last, "%1", "Continue?"));
        assert!(is_repeat(&mut last, "%1", "Continue?"));
        assert!(!is_repeat(&mut last, "%2", "Continue?"), "another pane");
        assert!(!is_repeat(&mut last, "%1", "Push it?"), "a new question");
        assert!(
            !is_repeat(&mut last, "%1", "Continue?"),
            "and the old one after it"
        );
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
        let plain = nudge_command(&e, 1300, &[], 4000);
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
            4000,
        );
        assert_eq!(custom, ["notify", "claude at api:2.1", "> Continue? (5m)"]);
    }

    #[test]
    fn a_nudge_can_stay_until_a_key_is_pressed() {
        let e = quiet(&pane("%1", "claude", 1000, false), "> Continue?");
        let cmd = nudge_command(&e, 1300, &[], 0);
        assert_eq!(cmd[2..4], ["-d", "0"]);
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
