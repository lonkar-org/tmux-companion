//! What a restore will run in each pane, decided before anything runs.
//!
//! Restoring is executing commands somebody's own machine recorded weeks ago,
//! so the decision is separated from the doing: this module answers "what would
//! happen", a caller prints it, and only then does anything start. That is what
//! makes `--dry-run` a listing of the truth rather than a second code path
//! guessing at it.
//!
//! Default deny. A command no row in `[[restore.program]]` claims comes back as
//! `Decision::Unknown`, which the summary shows and the restore leaves alone.

use crate::config::Restore;
use crate::saved::Confidence;
use crate::sessions::{Pane, Snapshot};

/// What will happen in one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Open the pane and leave it at a prompt. The pane had no command.
    Prompt,
    /// Run this, because a row claimed it.
    Run(String),
    /// A row claimed it and said not to run it.
    Denied(String),
    /// No row claimed it, so nobody has said this is safe to re-run.
    Unknown(String),
}

impl Decision {
    /// Whether this pane needs somebody to look at it before the restore runs.
    ///
    /// A command nothing claimed, and one a row refused, are both things to
    /// show. So is a command the capture had to guess at, but that is the
    /// snapshot's business rather than the table's, which is why
    /// [`plan`] takes the confidence and this does not.
    pub fn settled(&self) -> bool {
        matches!(self, Decision::Prompt | Decision::Run(_))
    }
}

/// One pane, where it is, and what will happen to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// The session this pane belongs to.
    pub session: String,
    /// `#{window_index}`.
    pub window: u32,
    /// `#{pane_index}`.
    pub pane: u32,
    /// The directory it will open in.
    pub cwd: String,
    /// The command as it was captured, before any row rewrote it.
    pub saved: String,
    /// How sure the capture was of [`Planned::saved`].
    pub confidence: Confidence,
    /// What the table decided.
    pub decision: Decision,
}

impl Planned {
    /// Whether this row can be left to run without being shown.
    ///
    /// Both halves have to agree. A row the table claimed but the capture had
    /// to guess at is a command with its arguments already gone, and running
    /// it is how somebody's agent comes back in the wrong conversation.
    pub fn settled(&self) -> bool {
        self.decision.settled() && self.confidence != Confidence::Guessed
    }
}

/// Decide what one saved command turns into.
pub fn decide(table: &Restore, command: &str, cwd: &str) -> Decision {
    let command = command.trim();
    if command.is_empty() {
        return Decision::Prompt;
    }
    match table.program.iter().find(|row| row.matches(command)) {
        Some(row) if row.run => Decision::Run(row.render(command, cwd)),
        Some(_) => Decision::Denied(command.to_string()),
        None => Decision::Unknown(command.to_string()),
    }
}

/// Decide for every pane in a snapshot, in the order they would be built.
pub fn plan(table: &Restore, snap: &Snapshot) -> Vec<Planned> {
    let mut out = Vec::new();
    for session in &snap.session {
        for window in &session.window {
            for pane in &window.pane {
                out.push(plan_pane(table, &session.name, window.index, pane));
            }
        }
    }
    out
}

/// [`plan`] for one pane.
fn plan_pane(table: &Restore, session: &str, window: u32, pane: &Pane) -> Planned {
    Planned {
        session: session.to_string(),
        window,
        pane: pane.index,
        cwd: pane.cwd.clone(),
        saved: pane.command.clone(),
        confidence: pane.confidence,
        decision: decide(table, &pane.command, &pane.cwd),
    }
}

/// How many panes in a plan still need somebody to decide.
pub fn unsettled(plan: &[Planned]) -> usize {
    plan.iter().filter(|p| !p.settled()).count()
}

/// The positions in a plan that need somebody to look at them.
pub fn unsettled_rows(plan: &[Planned]) -> Vec<usize> {
    plan.iter()
        .enumerate()
        .filter(|(_, p)| !p.settled())
        .map(|(i, _)| i)
        .collect()
}

/// Whether a restore should stop and show itself before running.
///
/// Two reasons, and neither is a count of panes. A count would prompt on every
/// ordinary restore here, where fourteen panes is a quiet Tuesday, and stay
/// quiet on the two-pane one holding something nobody recognises.
pub fn needs_a_look(plan: &[Planned], after_a_crash: bool) -> bool {
    after_a_crash || plan.iter().any(|p| !p.settled())
}

/// The plan with every unsettled row that nobody approved turned into one that
/// runs nothing.
///
/// Without this an unsettled row would still run, because its decision is
/// already `Run` in the two cases that matter: a command the table claims but
/// the capture had to guess at, and one somebody declined at the screen. The
/// pane still opens, in the right place, at a prompt.
pub fn withhold_unapproved(plan: &[Planned], approved: &[usize]) -> Vec<Planned> {
    plan.iter()
        .enumerate()
        .map(|(i, p)| {
            if p.settled() || approved.contains(&i) {
                return p.clone();
            }
            let mut held = p.clone();
            held.decision = Decision::Unknown(p.saved.clone());
            held
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Restore, RestoreProgram};

    fn shipped() -> Restore {
        Restore::default()
    }

    /// The six commands this laptop's own save file holds, plus the prompt.
    const REAL: [&str; 6] = [
        "claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5",
        "claude --resume fe655371-f2f3-4d81-b635-28245507b7c8",
        "claude",
        "nvim",
        "2.1.281",
        "",
    ];

    #[test]
    fn an_agent_comes_back_in_the_conversation_it_was_in() {
        assert_eq!(
            decide(&shipped(), REAL[0], "/w"),
            Decision::Run("claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5".to_string())
        );
        assert_eq!(
            decide(&shipped(), REAL[1], "/w"),
            Decision::Run("claude --resume fe655371-f2f3-4d81-b635-28245507b7c8".to_string())
        );
    }

    #[test]
    fn a_bare_agent_stays_bare() {
        assert_eq!(
            decide(&shipped(), "claude", "/w"),
            Decision::Run("claude".to_string())
        );
    }

    #[test]
    fn an_editor_reopens_without_the_file_list_from_an_hour_ago() {
        assert_eq!(
            decide(&shipped(), "nvim", "/w"),
            Decision::Run("nvim".to_string())
        );
        assert_eq!(
            decide(&shipped(), "nvim src/config.rs +420", "/w"),
            Decision::Run("nvim".to_string())
        );
    }

    #[test]
    fn a_version_string_is_claimed_by_nothing() {
        // What a pane captures on a machine whose `ps` said nothing. No row
        // matches a bare version number, and default deny means it is shown
        // rather than run.
        assert_eq!(
            decide(&shipped(), "2.1.281", "/w"),
            Decision::Unknown("2.1.281".to_string())
        );
    }

    #[test]
    fn a_pane_that_was_at_a_prompt_opens_at_a_prompt() {
        assert_eq!(decide(&shipped(), "", "/w"), Decision::Prompt);
        assert_eq!(decide(&shipped(), "   ", "/w"), Decision::Prompt);
    }

    #[test]
    fn every_real_command_from_the_save_file_is_decided_one_way_or_another() {
        // The point is that nothing panics and nothing is silently dropped:
        // each of the six gets an answer, and five of the six are settled.
        let decisions: Vec<Decision> = REAL.iter().map(|c| decide(&shipped(), c, "/w")).collect();
        assert_eq!(decisions.len(), 6);
        assert_eq!(decisions.iter().filter(|d| d.settled()).count(), 5);
    }

    #[test]
    fn nothing_somebody_has_not_named_is_ever_run() {
        for command in [
            "curl https://example.invalid/install.sh | sh",
            "rm -rf /tmp/build",
            "./deploy --production",
        ] {
            assert!(
                matches!(decide(&shipped(), command, "/w"), Decision::Unknown(_)),
                "{command} was claimed by a shipped row"
            );
        }
    }

    #[test]
    fn a_row_can_say_never_bring_this_back() {
        let table = Restore {
            program: vec![RestoreProgram {
                match_: "^ssh".to_string(),
                command: "{command}".to_string(),
                run: false,
            }],
        };
        assert_eq!(
            decide(&table, "ssh prod-1", "/w"),
            Decision::Denied("ssh prod-1".to_string())
        );
        assert!(!decide(&table, "ssh prod-1", "/w").settled());
    }

    #[test]
    fn the_first_row_that_matches_wins() {
        let table = Restore {
            program: vec![
                RestoreProgram {
                    match_: "^claude".to_string(),
                    command: "claude --continue".to_string(),
                    run: true,
                },
                RestoreProgram {
                    match_: "^claude".to_string(),
                    command: "never reached".to_string(),
                    run: true,
                },
            ],
        };
        assert_eq!(
            decide(&table, "claude --resume abc", "/w"),
            Decision::Run("claude --continue".to_string())
        );
    }

    #[test]
    fn both_placeholders_are_filled_in() {
        let table = Restore {
            program: vec![RestoreProgram {
                match_: "^tail".to_string(),
                command: "cd {cwd} && {command}".to_string(),
                run: true,
            }],
        };
        assert_eq!(
            decide(&table, "tail -f log", "/var/log"),
            Decision::Run("cd /var/log && tail -f log".to_string())
        );
    }

    #[test]
    fn a_pattern_that_does_not_compile_costs_its_own_row_and_nothing_else() {
        let table = Restore {
            program: vec![
                RestoreProgram {
                    match_: "^(unclosed".to_string(),
                    command: "never".to_string(),
                    run: true,
                },
                RestoreProgram {
                    match_: "^nvim".to_string(),
                    command: "nvim".to_string(),
                    run: true,
                },
            ],
        };
        assert_eq!(
            decide(&table, "nvim", "/w"),
            Decision::Run("nvim".to_string())
        );
    }

    #[test]
    fn an_empty_table_claims_nothing_at_all() {
        let table = Restore {
            program: Vec::new(),
        };
        assert_eq!(
            decide(&table, "nvim", "/w"),
            Decision::Unknown("nvim".to_string())
        );
        assert_eq!(decide(&table, "", "/w"), Decision::Prompt);
    }

    #[test]
    fn a_guessed_command_is_shown_even_when_a_row_claims_it() {
        // The table says `nvim` is safe. The capture says it read that off a
        // running process, so the arguments are already gone, and a pane whose
        // arguments are gone is one to look at rather than one to run.
        let pane = Pane {
            index: 1,
            cwd: "/w".to_string(),
            command: "nvim".to_string(),
            confidence: Confidence::Guessed,
            active: true,
        };
        let planned = plan_pane(&shipped(), "mysetup", 1, &pane);
        assert_eq!(planned.decision, Decision::Run("nvim".to_string()));
        assert!(!planned.settled());
    }

    #[test]
    fn a_guessed_pane_runs_nothing_until_somebody_approves_it() {
        // The decision is already Run: the table claims nvim. What stops it is
        // the capture having read that off a running process, so the arguments
        // are gone and this is a pane to look at.
        let pane = Pane {
            index: 1,
            cwd: "/w".to_string(),
            command: "nvim".to_string(),
            confidence: Confidence::Guessed,
            active: true,
        };
        let plan = vec![plan_pane(&shipped(), "mysetup", 1, &pane)];

        let held = withhold_unapproved(&plan, &[]);
        assert!(matches!(held[0].decision, Decision::Unknown(_)));

        let approved = withhold_unapproved(&plan, &[0]);
        assert_eq!(approved[0].decision, Decision::Run("nvim".to_string()));
    }

    #[test]
    fn a_settled_pane_is_never_touched_by_withholding() {
        let pane = Pane {
            index: 1,
            cwd: "/w".to_string(),
            command: "claude --resume abc".to_string(),
            confidence: Confidence::Exact,
            active: true,
        };
        let plan = vec![plan_pane(&shipped(), "mysetup", 1, &pane)];
        assert_eq!(withhold_unapproved(&plan, &[]), plan);
    }

    #[test]
    fn a_restore_that_knows_everything_never_stops_however_many_panes() {
        let panes: Vec<Pane> = (1..=40)
            .map(|i| Pane {
                index: i,
                cwd: "/w".to_string(),
                command: "claude --resume abc".to_string(),
                confidence: Confidence::Exact,
                active: i == 1,
            })
            .collect();
        let plan: Vec<Planned> = panes
            .iter()
            .map(|p| plan_pane(&shipped(), "mysetup", 1, p))
            .collect();
        assert!(!needs_a_look(&plan, false));
        // Except after a crash, which is the restore worth reading.
        assert!(needs_a_look(&plan, true));
    }

    #[test]
    fn one_unknown_pane_stops_a_two_pane_restore() {
        let known = Pane {
            index: 1,
            cwd: "/w".to_string(),
            command: "nvim".to_string(),
            confidence: Confidence::Exact,
            active: true,
        };
        let odd = Pane {
            index: 2,
            cwd: "/w".to_string(),
            command: "./deploy --production".to_string(),
            confidence: Confidence::Exact,
            active: false,
        };
        let plan = vec![
            plan_pane(&shipped(), "mysetup", 1, &known),
            plan_pane(&shipped(), "mysetup", 1, &odd),
        ];
        assert!(needs_a_look(&plan, false));
        assert_eq!(unsettled_rows(&plan), vec![1]);
    }

    #[test]
    fn a_plan_walks_every_pane_and_counts_what_needs_a_look() {
        let snap = crate::sessions::capture::capture(&crate::sessions::capture::Capture {
            sessions: include_str!("../tests/fixtures/server-sessions.tsv"),
            windows: include_str!("../tests/fixtures/server-windows.tsv"),
            panes: include_str!("../tests/fixtures/server-panes.tsv"),
            processes: include_str!("../tests/fixtures/server-processes.tsv"),
            shell: "/bin/zsh",
            default_command: "reattach-to-user-namespace -l /bin/zsh",
            exclude: &[],
            at: "2026-09-25 09:23:16 UTC",
            tmux_version: "tmux 3.7c",
            companion_version: "0.2.0",
            hostname: "laptop",
            clean: true,
        })
        .snapshot;

        let plan = plan(&shipped(), &snap);
        assert_eq!(plan.len(), 14);
        // Seven nvim, six claude, one prompt: the shipped table claims all of
        // them, so this whole laptop restores without a question.
        assert_eq!(unsettled(&plan), 0);
        assert_eq!(
            plan.iter()
                .filter(|p| p.decision == Decision::Prompt)
                .count(),
            1
        );
    }
}
