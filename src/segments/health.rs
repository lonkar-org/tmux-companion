//! One mark on the bar when the daemon knows something is wrong.
//!
//! The `[autosave]` timer failed every fifteen minutes for a day and nothing on
//! screen changed, because the only place a failure went was a log nobody
//! opened. The daemon is the one process that knows when a timer failed, when
//! the config it holds is older than the file on disk, and when the binary on
//! disk is newer than the one running; this segment draws a glyph and the first
//! reason's name when any of those is true, and nothing at all otherwise, so a
//! healthy bar carries nothing extra. `tmux-companion doctor` asks the daemon
//! the same question and prints every reason in full.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::panes::WAITING_COLOUR;
use crate::tmux::{format::colored_segment, icons::HEALTH};

/// How long a failed timer keeps the mark up after its last failure.
///
/// An hour, because the timers run every two to fifteen minutes and a failure
/// that has not repeated in an hour has been fixed, or the timer is off.
pub const FAILURE_WINDOW: Duration = Duration::from_secs(3600);

/// How often the daemon re-checks the two files.
///
/// Two stats every five seconds is nothing; every second per attached client
/// would be the kind of cost this tool exists to remove.
pub const INTERVAL: Duration = Duration::from_secs(5);

/// What the daemon found wrong, in the order the bar names them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HealthSample {
    /// One short line per reason; empty means healthy.
    pub reasons: Vec<String>,
}

/// The reasons, from three answers the daemon already has.
///
/// The failure comes first because it is the one that costs something now; a
/// changed config or a newer binary only means a restart is due.
pub fn reasons(failure: Option<&str>, config_changed: bool, binary_newer: bool) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(what) = failure {
        out.push(format!("a timer failed: {what}"));
    }
    if config_changed {
        out.push(
            "config.toml changed after the daemon started; run tmux-companion restart".to_string(),
        );
    }
    if binary_newer {
        out.push(
            "the binary on disk is newer than the running daemon; run tmux-companion restart"
                .to_string(),
        );
    }
    out
}

/// Whether a file was written after a moment.
///
/// A file that cannot be read has no opinion, so it is never newer: the mark
/// is for things the daemon knows, not for things it cannot find out.
pub fn newer_than(path: &Path, moment: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|mtime| mtime > moment)
        .unwrap_or(false)
}

/// The word the bar shows for a reason: what is wrong, not the whole sentence.
pub fn short_word(reason: &str) -> &'static str {
    if reason.starts_with("a timer failed") {
        "timer"
    } else if reason.starts_with("config.toml") {
        "config"
    } else if reason.starts_with("the binary") {
        "binary"
    } else {
        "check"
    }
}

/// The segment: the glyph and the first reason's word, `+N` when there are
/// more, or nothing.
pub fn format_health(sample: &HealthSample, bar_bg: &str) -> String {
    let Some(first) = sample.reasons.first() else {
        return String::new();
    };
    let more = sample.reasons.len() - 1;
    let text = if more == 0 {
        format!("{HEALTH}{}", short_word(first))
    } else {
        format!("{HEALTH}{} +{more}", short_word(first))
    };
    colored_segment(false, WAITING_COLOUR, bar_bg, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = "colour233";

    #[test]
    fn healthy_is_nothing_at_all() {
        assert_eq!(format_health(&HealthSample::default(), BAR), "");
        assert!(reasons(None, false, false).is_empty());
    }

    #[test]
    fn a_failure_comes_first_and_the_bar_says_which_kind() {
        let r = reasons(Some("autosave: script missing"), true, true);
        assert_eq!(r.len(), 3);
        assert!(r[0].contains("autosave: script missing"), "{}", r[0]);
        assert!(r[1].contains("config.toml"), "{}", r[1]);
        assert!(r[2].contains("binary"), "{}", r[2]);
        let drawn = format_health(&HealthSample { reasons: r }, BAR);
        assert!(drawn.contains("timer +2"), "{drawn}");
        assert!(drawn.contains(WAITING_COLOUR), "{drawn}");
    }

    #[test]
    fn one_reason_carries_no_count() {
        let sample = HealthSample {
            reasons: reasons(None, true, false),
        };
        let drawn = format_health(&sample, BAR);
        assert!(
            drawn.ends_with(&format!("{HEALTH}config")) || drawn.contains("config"),
            "{drawn}"
        );
        assert!(!drawn.contains('+'), "{drawn}");
    }

    #[test]
    fn a_file_is_newer_only_when_written_after_the_moment() {
        let dir = std::env::temp_dir().join(format!("tc-health-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        let before = SystemTime::now() - Duration::from_secs(60);
        std::fs::write(&file, "x").unwrap();
        assert!(newer_than(&file, before));
        let after = SystemTime::now() + Duration::from_secs(60);
        assert!(!newer_than(&file, after));
        assert!(!newer_than(&dir.join("absent"), before));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
