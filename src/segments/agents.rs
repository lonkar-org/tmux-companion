//! How many coding agents are running, and how many are busy or waiting on
//! you.
//!
//! The number somebody with four agents open wants on the bar is not four, it
//! is how many of the four need something, or, for somebody who keeps a dozen
//! open and works one at a time, how many are actually doing anything. So
//! the segment reads `4 agents · 1 waiting`, or `4 agents · 1 busy`, or both,
//! as `[agents] show` says, and it is the second half that is coloured.
//!
//! The rows come from the same `list-panes` the `panes` picker draws, with the
//! same words for the same states, so what the bar counts is what the picker
//! shows. The daemon reads that list at most once per `[agents] interval_secs`
//! whatever the number of attached clients, and not at all when no configured
//! segment asks for it.

use crate::config::{AgentsShow, AgentsStyle};
use crate::panes::{self, BUSY_COLOUR, Pane, Reports, WAITING_COLOUR};
use crate::tmux::{
    format::colored_segment,
    icons::{AGENT, AGENT_TO, BUSY, WAITING},
};

/// The colour of the count when nothing is waiting.
const QUIET_COLOUR: &str = crate::tmux::format::FG_GREY89;

/// What one read of the pane list found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentsSample {
    /// Panes running one of `[agents] programs`.
    pub total: usize,
    /// Of those, the ones that have stopped: asked, done, or quiet for at
    /// least `[agents] waiting_secs`.
    pub waiting: usize,
    /// Of those, the ones working. Not `total - waiting`: a pane somebody is
    /// reading is neither.
    pub busy: usize,
}

/// Count the agents in a listing at a given moment.
pub fn count(
    panes: &[Pane],
    programs: &[String],
    reports: &Reports,
    now: u64,
    waiting_secs: u64,
) -> AgentsSample {
    let mut sample = AgentsSample::default();
    for pane in panes {
        if !panes::is_agent(&pane.command, programs) {
            continue;
        }
        sample.total += 1;
        let state = panes::state(
            pane,
            true,
            reports.get(&pane.id).copied(),
            now,
            waiting_secs,
        );
        if state.is_waiting() {
            sample.waiting += 1;
        } else if state.is_busy() {
            sample.busy += 1;
        }
    }
    sample
}

/// Read the pane list and count.
pub async fn sample(programs: &[String], reports: &Reports, waiting_secs: u64) -> AgentsSample {
    let panes = panes::list().await;
    count(&panes, programs, reports, panes::now_secs(), waiting_secs)
}

/// The segment: `N agents`, then `· M busy`, `· M waiting`, or both, or
/// nothing at all.
///
/// Nothing rather than `0 agents` on purpose: a bar with no agents on it
/// should not carry a segment about them, and an empty render is what makes
/// the side drop this segment's separator along with it. A count of zero
/// drops its own part the same way: `4 agents` alone says every one is quiet
/// or every one is busy, whichever `show` asked about.
pub fn format_agents(
    sample: AgentsSample,
    bar_bg: &str,
    style: AgentsStyle,
    show: AgentsShow,
) -> String {
    let AgentsSample {
        total,
        waiting,
        busy,
    } = sample;
    if total == 0 {
        return String::new();
    }
    let (head, busy_part, waiting_part) = match style {
        AgentsStyle::Words => {
            let plural = if total == 1 { "" } else { "s" };
            (
                format!("{AGENT}{total} agent{plural}"),
                format!(" \u{b7} {busy} busy"),
                format!(" \u{b7} {waiting} waiting"),
            )
        }
        // `󰚩 3 󰦖 1  1`: the count beside the robot, a progress mark and
        // the busy count, an arrow, the waiting count beside an hourglass,
        // and nothing spelled out.
        AgentsStyle::Glyphs => (
            format!("{AGENT}{total}"),
            format!(" {BUSY}{busy}"),
            format!(" {AGENT_TO}{WAITING}{waiting}"),
        ),
    };
    let mut out = colored_segment(false, QUIET_COLOUR, bar_bg, &head);
    if show.busy() && busy > 0 {
        out.push_str(&colored_segment(false, BUSY_COLOUR, bar_bg, &busy_part));
    }
    if show.waiting() && waiting > 0 {
        out.push_str(&colored_segment(
            false,
            WAITING_COLOUR,
            bar_bg,
            &waiting_part,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::{Report, Reported};

    const BAR: &str = "colour233";

    fn sample(total: usize, waiting: usize, busy: usize) -> AgentsSample {
        AgentsSample {
            total,
            waiting,
            busy,
        }
    }

    #[test]
    fn the_glyph_style_is_the_robot_a_count_a_hand_and_a_count() {
        let drawn = format_agents(
            sample(2, 1, 0),
            BAR,
            AgentsStyle::Glyphs,
            AgentsShow::Waiting,
        );
        let plain: String = strip(&drawn);
        assert_eq!(plain, format!("{AGENT}2 {AGENT_TO}{WAITING}1"));
        assert!(
            !strip(&format_agents(
                sample(2, 0, 2),
                BAR,
                AgentsStyle::Glyphs,
                AgentsShow::Waiting
            ))
            .contains(WAITING)
        );
        assert_eq!(
            format_agents(sample(0, 0, 0), BAR, AgentsStyle::Glyphs, AgentsShow::Both),
            ""
        );
    }

    #[test]
    fn show_picks_which_count_follows_the_total() {
        let s = sample(5, 2, 1);
        let busy = strip(&format_agents(
            s,
            BAR,
            AgentsStyle::Glyphs,
            AgentsShow::Busy,
        ));
        assert_eq!(busy, format!("{AGENT}5 {BUSY}1"));
        let both = strip(&format_agents(
            s,
            BAR,
            AgentsStyle::Glyphs,
            AgentsShow::Both,
        ));
        assert_eq!(both, format!("{AGENT}5 {BUSY}1 {AGENT_TO}{WAITING}2"));
        let words = format_agents(s, BAR, AgentsStyle::Words, AgentsShow::Both);
        assert!(
            words.contains(&format!("#[fg={BUSY_COLOUR},bg={BAR}] \u{b7} 1 busy"))
                && words.ends_with(&format!("#[fg={WAITING_COLOUR},bg={BAR}] \u{b7} 2 waiting")),
            "{words:?}"
        );
        // Nothing busy: the busy part goes, the way the waiting part does.
        let none = format_agents(sample(5, 5, 0), BAR, AgentsStyle::Words, AgentsShow::Busy);
        assert_eq!(
            none,
            format!("#[fg={QUIET_COLOUR},bg={BAR}]{AGENT}5 agents")
        );
    }

    /// The text without the tmux colour markup.
    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut skip = false;
        for c in s.chars() {
            match c {
                '#' => {}
                '[' if !skip => skip = true,
                ']' if skip => skip = false,
                _ if !skip => out.push(c),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn no_agents_is_no_segment() {
        assert_eq!(
            format_agents(
                sample(0, 0, 0),
                BAR,
                AgentsStyle::Words,
                AgentsShow::Waiting
            ),
            ""
        );
    }

    #[test]
    fn a_count_with_nothing_waiting_is_one_colour() {
        let out = format_agents(
            sample(3, 0, 3),
            BAR,
            AgentsStyle::Words,
            AgentsShow::Waiting,
        );
        assert_eq!(out, format!("#[fg={QUIET_COLOUR},bg={BAR}]{AGENT}3 agents"));
        assert!(!out.contains("waiting"));
    }

    #[test]
    fn one_agent_is_singular() {
        assert!(
            format_agents(
                sample(1, 0, 1),
                BAR,
                AgentsStyle::Words,
                AgentsShow::Waiting
            )
            .ends_with("1 agent")
        );
    }

    #[test]
    fn the_waiting_count_is_the_part_that_stands_out() {
        let out = format_agents(
            sample(4, 1, 3),
            BAR,
            AgentsStyle::Words,
            AgentsShow::Waiting,
        );
        assert!(out.starts_with(&format!("#[fg={QUIET_COLOUR},bg={BAR}]{AGENT}4 agents")));
        assert!(
            out.ends_with(&format!("#[fg={WAITING_COLOUR},bg={BAR}] \u{b7} 1 waiting")),
            "{out:?}"
        );
    }

    #[test]
    fn the_bar_background_is_the_one_given() {
        assert!(
            format_agents(
                sample(1, 1, 0),
                "#121212",
                AgentsStyle::Words,
                AgentsShow::Waiting
            )
            .contains("bg=#121212")
        );
    }

    #[test]
    fn counting_uses_the_configured_list_the_threshold_and_the_reports() {
        let listing = "\
api\t1\t0\t0\t%1\tclaude\t/a\tlaptop\t1\t1\t1\t995\t0\tlaptop\t0
api\t2\t0\t0\t%2\tclaude\t/a\tlaptop\t1\t1\t1\t800\t0\tlaptop\t0
web\t1\t0\t0\t%3\tzsh\t/w\tlaptop\t1\t1\t1\t0\t0\tlaptop\t0
web\t1\t0\t1\t%4\tcodex\t/w\tlaptop\t1\t1\t1\t0\t1\tlaptop\t0
";
        let panes = panes::parse(listing);
        let programs = crate::config::Agents::default().programs;
        let none = Reports::new();
        // Three agents; one is busy, one has been quiet 200s, and one is in
        // copy mode, which is somebody reading it rather than it waiting.
        assert_eq!(count(&panes, &programs, &none, 1_000, 10), sample(3, 1, 1));
        assert_eq!(
            count(&panes, &[], &none, 1_000, 10),
            AgentsSample::default()
        );
        // The quiet one said it is busy, and the drawing one said it asked.
        let reports = Reports::from([
            (
                "%2".to_string(),
                Reported {
                    state: Report::Busy,
                    at: 900,
                },
            ),
            (
                "%1".to_string(),
                Reported {
                    state: Report::Asked,
                    at: 990,
                },
            ),
        ]);
        assert_eq!(
            count(&panes, &programs, &reports, 1_000, 10),
            sample(3, 1, 1)
        );
    }
}
