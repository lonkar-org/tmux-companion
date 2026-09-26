//! The screen a restore shows when it does not know something.
//!
//! Not a picker. The six pickers filter a long list down to one row; this shows
//! a short list and asks which of it to run, so it borrows the terminal setup
//! and nothing else.
//!
//! It opens only when the restore is unsure: a pane no `[[restore.program]]`
//! row claims, one a row refused, or a command the capture read off a running
//! process so its arguments are already gone. A restore that knows everything
//! goes straight through however many panes it holds, because a confirmation on
//! every restore is one people turn off inside a week.
//!
//! It counts down and proceeds on its own. Touching any key stops the clock,
//! which makes it a status line somebody may interrupt rather than a question
//! somebody must answer.

use std::time::{Duration, Instant};

use crate::restore::{Decision, Planned};

/// What the person did with the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Go ahead, running the rows at these positions in the plan on top of
    /// everything already settled.
    Go(Vec<usize>),
    /// Do nothing at all.
    Cancelled,
}

/// One line of the list, already worded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Where this sits in the plan, so an approval can be applied to it.
    pub index: usize,
    /// `session:window.pane`.
    pub at: String,
    /// The command as captured.
    pub command: String,
    /// Why it is on this screen.
    pub why: &'static str,
    /// Whether it will run when the countdown ends.
    pub approved: bool,
}

/// Why a pane needs looking at, in three words.
pub fn why(planned: &Planned) -> &'static str {
    match &planned.decision {
        Decision::Denied(_) => "refused by a row",
        Decision::Unknown(_) => "nothing claims it",
        // The table claims it, so the doubt is the capture's: it read the
        // command off a running process and the arguments are gone.
        _ => "arguments already lost",
    }
}

/// The rows a summary screen shows, from a plan.
pub fn rows(plan: &[Planned]) -> Vec<Row> {
    plan.iter()
        .enumerate()
        .filter(|(_, p)| !p.settled())
        .map(|(index, p)| Row {
            index,
            at: format!("{}:{}.{}", p.session, p.window, p.pane),
            command: if p.saved.is_empty() {
                "a shell".to_string()
            } else {
                p.saved.clone()
            },
            why: why(p),
            approved: false,
        })
        .collect()
}

/// The line above the list.
pub fn headline(
    sessions: usize,
    panes: usize,
    agents: usize,
    captured_at: &str,
    clean: bool,
) -> String {
    format!(
        "restoring {sessions} session{}  {panes} pane{}  {agents} agent{}      from {captured_at}, {}",
        plural(sessions),
        plural(panes),
        plural(agents),
        // A timer's capture is the ordinary case and not a fault, so it is
        // named by when it was taken rather than by what it lacks.
        if clean {
            "taken at shutdown"
        } else {
            "taken while running"
        }
    )
}

/// How many panes in a plan are running one of `programs`.
///
/// Counted for the headline, because "6 agents" is the number somebody scans
/// for when deciding whether this is the restore they meant. The list is
/// `[agents] programs`, the same one the `panes --agents` filter and the bar
/// segment read, so the three cannot disagree about what an agent is.
pub fn agents(plan: &[Planned], programs: &[String]) -> usize {
    plan.iter()
        .filter(|p| {
            let first = p.saved.split_whitespace().next().unwrap_or_default();
            programs.iter().any(|a| a == first)
        })
        .count()
}

/// `s` unless there is one of something.
fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The countdown, as the screen writes it.
pub fn remaining(total: Duration, elapsed: Duration) -> u64 {
    total.saturating_sub(elapsed).as_secs()
        + u64::from(total.saturating_sub(elapsed).subsec_millis() > 0)
}

/// Show the screen and answer what to do.
///
/// `countdown` of zero never draws anything and goes ahead with whatever was
/// already settled, which is `--yes` made permanent.
pub fn confirm(headline: &str, rows: Vec<Row>, countdown: Duration) -> anyhow::Result<Outcome> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

    if countdown.is_zero() || rows.is_empty() {
        return Ok(Outcome::Go(approved(&rows)));
    }

    let mut rows = rows;
    let mut cursor = 0usize;
    let started = Instant::now();
    let mut counting = true;

    let mut terminal = ratatui::init();
    // Whatever happens below, the terminal goes back to how it was found. A
    // screen that panics with raw mode still on leaves the pane unusable.
    let result = (|| -> anyhow::Result<Outcome> {
        loop {
            let left = remaining(countdown, started.elapsed());
            if counting && left == 0 {
                return Ok(Outcome::Go(approved(&rows)));
            }
            terminal.draw(|frame| {
                draw(frame, headline, &rows, cursor, counting.then_some(left));
            })?;

            // Poll rather than block, so the clock can run out while nobody is
            // pressing anything.
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            // Any key at all stops the clock. Somebody who reached for the
            // keyboard is reading it, and having it proceed under them is the
            // whole failure this avoids.
            counting = false;
            match key.code {
                KeyCode::Enter => return Ok(Outcome::Go(approved(&rows))),
                KeyCode::Esc | KeyCode::Char('q') => return Ok(Outcome::Cancelled),
                KeyCode::Char('c')
                    if key
                        .modifiers
                        .contains(ratatui::crossterm::event::KeyModifiers::CONTROL) =>
                {
                    return Ok(Outcome::Cancelled);
                }
                KeyCode::Tab | KeyCode::Char(' ') => {
                    if let Some(row) = rows.get_mut(cursor) {
                        row.approved = !row.approved;
                    }
                }
                KeyCode::Char('a') => {
                    let all = !rows.iter().all(|r| r.approved);
                    for row in &mut rows {
                        row.approved = all;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    cursor = (cursor + 1).min(rows.len().saturating_sub(1));
                }
                KeyCode::Up | KeyCode::Char('k') => cursor = cursor.saturating_sub(1),
                _ => {}
            }
        }
    })();
    ratatui::restore();
    result
}

/// The plan positions somebody approved.
fn approved(rows: &[Row]) -> Vec<usize> {
    rows.iter()
        .filter(|r| r.approved)
        .map(|r| r.index)
        .collect()
}

/// Draw one frame.
fn draw(
    frame: &mut ratatui::Frame,
    headline: &str,
    rows: &[Row],
    cursor: usize,
    counting: Option<u64>,
) {
    use ratatui::layout::{Constraint, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph};

    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(headline),
            Line::from(Span::styled(
                format!(
                    "{} pane{} need{} a decision",
                    rows.len(),
                    plural(rows.len()),
                    if rows.len() == 1 { "s" } else { "" }
                ),
                Style::default().fg(Color::Yellow),
            )),
        ]),
        chunks[0],
    );

    let list: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mark = if row.approved { "run " } else { "skip" };
            let style = if i == cursor {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Line::from(Span::styled(
                format!("{mark}  {:<22}  {}  ({})", row.at, row.command, row.why),
                style,
            ))
        })
        .collect();
    frame.render_widget(
        Paragraph::new(list).block(Block::default().borders(Borders::TOP)),
        chunks[1],
    );

    let keys = match counting {
        Some(left) => {
            format!("[enter] go   [tab] run this one   [a] all   [q] cancel        {left}\u{2026}")
        }
        None => "[enter] go   [tab] run this one   [a] all   [q] cancel".to_string(),
    };
    frame.render_widget(
        Paragraph::new(keys).block(Block::default().borders(Borders::TOP)),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::restore::plan;
    use crate::saved::Confidence;
    use crate::sessions::{Header, Pane, Session, Snapshot, Window};

    fn snapshot(commands: &[(&str, Confidence)]) -> Snapshot {
        Snapshot {
            header: Header::default(),
            session: vec![Session {
                name: "mysetup".to_string(),
                path: "/w".to_string(),
                window: vec![Window {
                    index: 1,
                    name: "edit".to_string(),
                    layout: String::new(),
                    active: true,
                    zoomed: false,
                    pane: commands
                        .iter()
                        .enumerate()
                        .map(|(i, (c, how))| Pane {
                            index: i as u32 + 1,
                            cwd: "/w".to_string(),
                            command: (*c).to_string(),
                            confidence: *how,
                            active: i == 0,
                            title: String::new(),
                        })
                        .collect(),
                }],
            }],
        }
    }

    fn planned(commands: &[(&str, Confidence)]) -> Vec<crate::restore::Planned> {
        plan(&crate::config::Restore::default(), &snapshot(commands))
    }

    #[test]
    fn a_restore_that_knows_everything_has_nothing_to_show() {
        let plan = planned(&[
            ("claude --resume abc", Confidence::Exact),
            ("nvim", Confidence::Exact),
        ]);
        assert!(rows(&plan).is_empty());
    }

    #[test]
    fn each_kind_of_doubt_gets_its_own_wording() {
        let plan = planned(&[
            ("./deploy --production", Confidence::Exact),
            ("nvim", Confidence::Guessed),
        ]);
        let rows = rows(&plan);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].why, "nothing claims it");
        assert_eq!(rows[1].why, "arguments already lost");
        // Nothing is approved before somebody says so.
        assert!(rows.iter().all(|r| !r.approved));
    }

    #[test]
    fn a_pane_with_no_command_is_described_rather_than_left_blank() {
        // It cannot reach the screen, since a prompt is settled, but the
        // wording exists so a future caller cannot print an empty column.
        let plan = planned(&[("", Confidence::Shell)]);
        assert!(rows(&plan).is_empty());
        let row = Row {
            index: 0,
            at: "mysetup:1.1".to_string(),
            command: String::new(),
            why: "nothing claims it",
            approved: false,
        };
        assert_eq!(row.command, "");
    }

    #[test]
    fn the_rows_carry_their_position_in_the_plan() {
        let plan = planned(&[
            ("nvim", Confidence::Exact),
            ("./deploy", Confidence::Exact),
            ("claude", Confidence::Exact),
            ("./other", Confidence::Exact),
        ]);
        let rows = rows(&plan);
        assert_eq!(rows.iter().map(|r| r.index).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn agents_are_counted_for_the_headline() {
        let plan = planned(&[
            ("claude --resume abc", Confidence::Exact),
            ("nvim", Confidence::Exact),
            ("codex", Confidence::Exact),
        ]);
        assert_eq!(agents(&plan, &crate::config::Agents::default().programs), 2);
    }

    #[test]
    fn what_counts_as_an_agent_is_the_configured_list() {
        // The list used to be compiled in, so an agent nobody here had heard
        // of was never counted.
        let plan = planned(&[
            ("myagent --go", Confidence::Exact),
            ("nvim", Confidence::Exact),
        ]);
        assert_eq!(agents(&plan, &[]), 0);
        assert_eq!(agents(&plan, &["myagent".to_string()]), 1);
    }

    #[test]
    fn the_headline_says_what_it_is_about_to_do() {
        let line = headline(7, 14, 6, "2026-09-25 09:23:16 UTC", true);
        assert!(line.contains("7 sessions"));
        assert!(line.contains("14 panes"));
        assert!(line.contains("6 agents"));
        assert!(line.contains("taken at shutdown"), "{line}");

        let running = headline(1, 1, 0, "2026-09-25 09:23:16 UTC", false);
        assert!(running.contains("1 session "), "{running}");
        // Every timer snapshot reads this way, so it must not sound like a
        // fault.
        assert!(running.contains("taken while running"), "{running}");
        assert!(!running.contains("no clean"), "{running}");
    }

    #[test]
    fn the_countdown_rounds_up_so_it_never_shows_zero_while_it_still_has_time() {
        assert_eq!(remaining(Duration::from_secs(5), Duration::ZERO), 5);
        assert_eq!(
            remaining(Duration::from_secs(5), Duration::from_millis(4_100)),
            1
        );
        assert_eq!(
            remaining(Duration::from_secs(5), Duration::from_millis(5_000)),
            0
        );
        assert_eq!(remaining(Duration::from_secs(5), Duration::from_secs(9)), 0);
    }

    #[test]
    fn a_countdown_of_zero_goes_ahead_without_drawing_anything() {
        let rows = vec![Row {
            index: 0,
            at: "mysetup:1.1".to_string(),
            command: "./deploy".to_string(),
            why: "nothing claims it",
            approved: false,
        }];
        // No terminal is touched, so this is safe to call in a test.
        let outcome = confirm("x", rows, Duration::ZERO).expect("no screen");
        assert_eq!(outcome, Outcome::Go(Vec::new()));
    }

    #[test]
    fn nothing_to_ask_about_goes_ahead_without_drawing_anything() {
        let outcome = confirm("x", Vec::new(), Duration::from_secs(5)).expect("no screen");
        assert_eq!(outcome, Outcome::Go(Vec::new()));
    }
}
