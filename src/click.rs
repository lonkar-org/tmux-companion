//! A mouse click on a segment of the bar.
//!
//! tmux's window list is clickable out of the box because tmux draws it with
//! `range=window|N` marks. The right side is ours to draw, so the segments
//! that have somewhere to go carry a `range=user|NAME` mark, and a
//! `MouseDown1StatusRight` binding in tmux.conf hands the name to this
//! command, which runs what `[[status.right.segments]] on_click` says or the
//! default for that segment: the inbox for `agents`, or the agent list when
//! `[agents] show` is `busy` and the number on the bar is the working ones,
//! which the inbox does not hold; the brief for `health`. A segment with
//! nothing to do on a click does nothing, which is what a click on the clock
//! has always done.

use crate::config::{AgentsShow, RightSegment, SegmentName};

/// The tmux command a click on `range` runs, when there is one.
///
/// `range` is what `#{mouse_status_range}` gives for a `range=user|X` mark:
/// the bare name. `me` is this binary by its own path, since a popup's shell
/// has the tmux server's PATH and may not find the bare name.
pub fn action(
    range: &str,
    segments: &[RightSegment],
    show: AgentsShow,
    me: &str,
    borderless: bool,
) -> Option<String> {
    // The pickers draw their own border, so on a tmux that can leave its own
    // off (3.3, `-B`) it does. The brief prints rather than draws, and
    // tmux's border is the only frame it has.
    let b = if borderless { "-B " } else { "" };
    let name = range.trim().strip_prefix("user|").unwrap_or(range.trim());
    let segment = segments.iter().find(|s| s.name.range_name() == name)?;
    if !segment.on_click.trim().is_empty() {
        return Some(segment.on_click.replace("{me}", me));
    }
    match segment.name {
        // The number clicked is the busy count, and the inbox lists the
        // stopped ones: the agent list is what that number is made of.
        SegmentName::Agents if show == AgentsShow::Busy => Some(format!(
            "display-popup {b}-E -w 80% -h 70% \"{me} panes --agents\""
        )),
        SegmentName::Agents => Some(format!("display-popup {b}-E -w 80% -h 70% \"{me} inbox\"")),
        SegmentName::Health => Some(format!("display-popup -E -w 70% -h 60% \"{me} brief\"")),
        _ => None,
    }
}

/// `click`: run what a click on that segment means.
pub async fn run(range: String) -> anyhow::Result<()> {
    let config = crate::cli::config_or_default();
    let me = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "tmux-companion".to_string());
    if let Some(cmd) = action(
        &range,
        &config.status.right.segments,
        config.agents.show,
        &me,
        crate::setup::catalog::popups_take_b(),
    ) {
        // `run-shell -C` runs a tmux command rather than a shell one, so the
        // config writes `display-popup ...` the way tmux.conf would.
        crate::cli::tmux(&["run-shell", "-C", &cmd]).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(name: SegmentName, on_click: &str) -> RightSegment {
        RightSegment {
            name,
            separator_before: String::new(),
            on_click: on_click.to_string(),
        }
    }

    #[test]
    fn agents_opens_the_inbox_and_health_the_brief_by_default() {
        let segs = vec![
            seg(SegmentName::Git, ""),
            seg(SegmentName::Agents, ""),
            seg(SegmentName::Health, ""),
        ];
        let a = action("agents", &segs, AgentsShow::Waiting, "/opt/tc", true).unwrap();
        assert!(
            a.starts_with("display-popup") && a.contains("/opt/tc inbox"),
            "{a}"
        );
        let h = action("user|health", &segs, AgentsShow::Waiting, "/opt/tc", true).unwrap();
        assert!(h.contains("/opt/tc brief"), "{h}");
        assert_eq!(
            action("git", &segs, AgentsShow::Waiting, "/opt/tc", true),
            None,
            "nothing to open for git"
        );
        assert_eq!(
            action("clock", &segs, AgentsShow::Waiting, "/opt/tc", true),
            None
        );
    }

    #[test]
    fn the_busy_count_opens_the_agent_list_rather_than_the_inbox() {
        let segs = vec![seg(SegmentName::Agents, "")];
        let a = action("agents", &segs, AgentsShow::Busy, "/opt/tc", true).unwrap();
        assert!(a.contains("/opt/tc panes --agents"), "{a}");
        let b = action("agents", &segs, AgentsShow::Both, "/opt/tc", true).unwrap();
        assert!(
            b.contains("/opt/tc inbox"),
            "both draws the waiting count too: {b}"
        );
    }

    #[test]
    fn a_configured_click_wins_and_can_name_this_binary() {
        let segs = vec![seg(SegmentName::Git, "display-popup -E \"{me} panes\"")];
        assert_eq!(
            action("git", &segs, AgentsShow::Waiting, "/opt/tc", true).as_deref(),
            Some("display-popup -E \"/opt/tc panes\"")
        );
    }
}
