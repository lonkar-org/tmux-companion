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

use crate::config::AgentsStyle;
use crate::panes::{self, Pane, WAITING_COLOUR};
use crate::tmux::{
    format::colored_segment,
    icons::{AGENT, WAITING},
};

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
pub fn format_agents(total: usize, waiting: usize, bar_bg: &str, style: AgentsStyle) -> String {
    if total == 0 {
        return String::new();
    }
    let (quiet, loud) = match style {
        AgentsStyle::Words => {
            let plural = if total == 1 { "" } else { "s" };
            (
                format!("{AGENT}{total} agent{plural}"),
                format!(" \u{b7} {waiting} waiting"),
            )
        }
        // `󰚩 2   1`: the count beside the robot, the waiting count beside a
        // raised hand, and nothing spelled out.
        AgentsStyle::Glyphs => (format!("{AGENT}{total}"), format!("  {WAITING}{waiting}")),
    };
    let mut out = colored_segment(false, QUIET_COLOUR, bar_bg, &quiet);
    if waiting > 0 {
        out.push_str(&colored_segment(false, WAITING_COLOUR, bar_bg, &loud));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = "colour233";

    #[test]
    fn the_glyph_style_is_the_robot_a_count_a_hand_and_a_count() {
        let drawn = format_agents(2, 1, BAR, AgentsStyle::Glyphs);
        let plain: String = strip(&drawn);
        assert_eq!(plain, format!("{AGENT}2  {WAITING}1"));
        assert!(!strip(&format_agents(2, 0, BAR, AgentsStyle::Glyphs)).contains(WAITING));
        assert_eq!(format_agents(0, 0, BAR, AgentsStyle::Glyphs), "");
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
        assert_eq!(format_agents(0, 0, BAR, AgentsStyle::Words), "");
    }

    #[test]
    fn a_count_with_nothing_waiting_is_one_colour() {
        let out = format_agents(3, 0, BAR, AgentsStyle::Words);
        assert_eq!(out, format!("#[fg={QUIET_COLOUR},bg={BAR}]{AGENT}3 agents"));
        assert!(!out.contains("waiting"));
    }

    #[test]
    fn one_agent_is_singular() {
        assert!(format_agents(1, 0, BAR, AgentsStyle::Words).ends_with("1 agent"));
    }

    #[test]
    fn the_waiting_count_is_the_part_that_stands_out() {
        let out = format_agents(4, 1, BAR, AgentsStyle::Words);
        assert!(out.starts_with(&format!("#[fg={QUIET_COLOUR},bg={BAR}]{AGENT}4 agents")));
        assert!(
            out.ends_with(&format!("#[fg={WAITING_COLOUR},bg={BAR}] \u{b7} 1 waiting")),
            "{out:?}"
        );
    }

    #[test]
    fn the_bar_background_is_the_one_given() {
        assert!(format_agents(1, 1, "#121212", AgentsStyle::Words).contains("bg=#121212"));
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
