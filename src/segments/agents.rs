//! How many coding agents are running, and how many are waiting on you.
//!
//! The number somebody with four agents open wants on the bar is not four, it
//! is how many of the four have stopped. One that is busy needs nothing; one
//! that has been quiet for ten seconds has very likely asked a question and is
//! sitting behind a window nobody is looking at. So the segment reads
//! `4 agents · 1 waiting`, and it is the second half that is coloured.
//!
//! The rows come from the same `list-panes` the `panes` picker draws, with the
//! same words for the same states, so what the bar counts is what the picker
//! shows. The daemon reads that list at most once per `[agents] interval_secs`
//! whatever the number of attached clients, and not at all when no configured
//! segment asks for it.

use crate::panes::{self, Pane, WAITING_COLOUR};
use crate::tmux::{format::colored_segment, icons::AGENT};

/// The colour of the count when nothing is waiting.
const QUIET_COLOUR: &str = crate::tmux::format::FG_GREY89;

/// What one read of the pane list found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentsSample {
    /// Panes running one of `[agents] programs`.
    pub total: usize,
    /// Of those, the ones quiet for at least `[agents] waiting_secs`.
    pub waiting: usize,
}

/// Count the agents in a listing at a given moment.
pub fn count(panes: &[Pane], programs: &[String], now: u64, waiting_secs: u64) -> AgentsSample {
    let mut sample = AgentsSample::default();
    for pane in panes {
        if !panes::is_agent(&pane.command, programs) {
            continue;
        }
        sample.total += 1;
        if panes::state(pane, true, now, waiting_secs).is_waiting() {
            sample.waiting += 1;
        }
    }
    sample
}

/// Read the pane list and count.
pub async fn sample(programs: &[String], waiting_secs: u64) -> AgentsSample {
    let panes = panes::list().await;
    count(&panes, programs, panes::now_secs(), waiting_secs)
}

/// The segment: `N agents`, or `N agents · M waiting`, or nothing at all.
///
/// Nothing rather than `0 agents` on purpose: a bar with no agents on it
/// should not carry a segment about them, and an empty render is what makes
/// the side drop this segment's separator along with it.
pub fn format_agents(total: usize, waiting: usize, bar_bg: &str) -> String {
    if total == 0 {
        return String::new();
    }
    let plural = if total == 1 { "" } else { "s" };
    let mut out = colored_segment(
        false,
        QUIET_COLOUR,
        bar_bg,
        &format!("{AGENT}{total} agent{plural}"),
    );
    if waiting > 0 {
        out.push_str(&colored_segment(
            false,
            WAITING_COLOUR,
            bar_bg,
            &format!(" \u{b7} {waiting} waiting"),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = "colour233";

    #[test]
    fn no_agents_is_no_segment() {
        assert_eq!(format_agents(0, 0, BAR), "");
    }

    #[test]
    fn a_count_with_nothing_waiting_is_one_colour() {
        let out = format_agents(3, 0, BAR);
        assert_eq!(out, format!("#[fg={QUIET_COLOUR},bg={BAR}]{AGENT}3 agents"));
        assert!(!out.contains("waiting"));
    }

    #[test]
    fn one_agent_is_singular() {
        assert!(format_agents(1, 0, BAR).ends_with("1 agent"));
    }

    #[test]
    fn the_waiting_count_is_the_part_that_stands_out() {
        let out = format_agents(4, 1, BAR);
        assert!(out.starts_with(&format!("#[fg={QUIET_COLOUR},bg={BAR}]{AGENT}4 agents")));
        assert!(
            out.ends_with(&format!("#[fg={WAITING_COLOUR},bg={BAR}] \u{b7} 1 waiting")),
            "{out:?}"
        );
    }

    #[test]
    fn the_bar_background_is_the_one_given() {
        assert!(format_agents(1, 1, "#121212").contains("bg=#121212"));
    }

    #[test]
    fn counting_uses_the_configured_list_and_the_waiting_threshold() {
        let listing = "\
api\t1\t0\t0\t%1\tclaude\t/a\tlaptop\t1\t1\t1\t995\t0\tlaptop
api\t2\t0\t0\t%2\tclaude\t/a\tlaptop\t1\t1\t1\t800\t0\tlaptop
web\t1\t0\t0\t%3\tzsh\t/w\tlaptop\t1\t1\t1\t0\t0\tlaptop
web\t1\t0\t1\t%4\tcodex\t/w\tlaptop\t1\t1\t1\t0\t1\tlaptop
";
        let panes = panes::parse(listing);
        let programs = crate::config::Agents::default().programs;
        let sample = count(&panes, &programs, 1_000, 10);
        // Three agents; one is busy, one has been quiet 200s, and one is in
        // copy mode, which is somebody reading it rather than it waiting.
        assert_eq!(
            sample,
            AgentsSample {
                total: 3,
                waiting: 1
            }
        );
        assert_eq!(count(&panes, &[], 1_000, 10), AgentsSample::default());
    }
}
