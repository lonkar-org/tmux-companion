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
//!
//! A failure leaves the mark three ways: an hour passes, the timer it came
//! from runs and works, or `tmux-companion health ack` forgets it. The other
//! reasons are how things are and leave when that changes.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::panes::WAITING_COLOUR;
use crate::tmux::icons::{HEALTH, QUIET};

/// How long a failed timer keeps the mark up after its last failure.
///
/// An hour, because the timers run every two to fifteen minutes and a failure
/// that has not repeated in an hour has been fixed, or the timer is off. It
/// is the longest a failure stays, for a segment or a timer that never runs
/// again; a timer that works takes its own back sooner.
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
pub fn reasons(failures: &[String], config_changed: bool, binary_newer: bool) -> Vec<String> {
    let mut out: Vec<String> = failures
        .iter()
        .map(|what| format!("a timer failed: {what}"))
        .collect();
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

/// What `health` prints: the reasons, one a line, or `ok`.
pub fn report(reasons: &[String]) -> String {
    if reasons.is_empty() {
        "ok".to_string()
    } else {
        reasons.join("\n")
    }
}

/// What `health ack` prints: what was forgotten, and what the mark still says.
///
/// What is left is said because an acknowledgement that leaves the mark up
/// with no word about why looks like a command that did nothing. Those are
/// the reasons that describe how things are now, and each one already names
/// what ends it.
pub fn ack_report(forgotten: &[String], left: &[String]) -> String {
    let mut lines: Vec<String> = if forgotten.is_empty() {
        vec!["nothing to acknowledge".to_string()]
    } else {
        forgotten
            .iter()
            .map(|what| format!("acknowledged: {what}"))
            .collect()
    };
    lines.extend(left.iter().map(|why| format!("still on the mark: {why}")));
    lines.join("\n")
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
    if crate::quiet::is_status_line(reason) {
        "quiet"
    } else if reason.starts_with("offline for") {
        "offline"
    } else if reason.starts_with("a timer failed") {
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
///
/// Only the foreground is set. The mark sits on whatever background the
/// segment before it left, so after the battery's block it stays in that
/// block; a `bg=` of its own put a strip of bar colour after the battery that
/// looked like a gap.
pub fn format_health(sample: &HealthSample) -> String {
    let Some(first) = sample.reasons.first() else {
        return String::new();
    };
    let more = sample.reasons.len() - 1;
    // Quiet is the bell crossed out and nothing else: the word said what the
    // glyph already does, at a quarter of the bar's width. `health` and
    // `quiet --status` still say it in words.
    let head = match short_word(first) {
        "quiet" => QUIET.to_string(),
        word => format!("{HEALTH}{word}"),
    };
    let text = if more == 0 {
        head
    } else {
        format!("{head} +{more}")
    };
    format!("#[fg={WAITING_COLOUR}]{text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_is_nothing_at_all() {
        assert_eq!(format_health(&HealthSample::default()), "");
        assert!(reasons(&[], false, false).is_empty());
    }

    #[test]
    fn a_failure_comes_first_and_the_bar_says_which_kind() {
        let r = reasons(&["autosave: script missing".to_string()], true, true);
        assert_eq!(r.len(), 3);
        assert!(r[0].contains("autosave: script missing"), "{}", r[0]);
        assert!(r[1].contains("config.toml"), "{}", r[1]);
        assert!(r[2].contains("binary"), "{}", r[2]);
        let drawn = format_health(&HealthSample { reasons: r });
        assert!(drawn.contains("timer +2"), "{drawn}");
        assert!(drawn.contains(WAITING_COLOUR), "{drawn}");
    }

    #[test]
    fn two_things_that_failed_are_two_reasons() {
        let failed = [
            "sessions autosave: disk full".to_string(),
            "net segment: no counters".to_string(),
        ];
        let r = reasons(&failed, false, false);
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].ends_with("sessions autosave: disk full"), "{}", r[0]);
        assert!(r[1].ends_with("net segment: no counters"), "{}", r[1]);
        let drawn = format_health(&HealthSample { reasons: r });
        assert!(drawn.contains("timer +1"), "{drawn}");
    }

    #[test]
    fn health_says_ok_or_the_reasons_one_a_line() {
        assert_eq!(report(&[]), "ok");
        let r = reasons(&["autosave: script missing".to_string()], true, false);
        let said = report(&r);
        assert_eq!(said.lines().count(), 2, "{said}");
        assert!(said.starts_with("a timer failed: autosave"), "{said}");
    }

    #[test]
    fn an_acknowledgement_says_what_it_forgot_and_what_it_could_not() {
        assert_eq!(ack_report(&[], &[]), "nothing to acknowledge");
        let forgotten = ["sessions autosave: disk full".to_string()];
        assert_eq!(
            ack_report(&forgotten, &[]),
            "acknowledged: sessions autosave: disk full"
        );
        let left = reasons(&[], true, false);
        let said = ack_report(&forgotten, &left);
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(lines.len(), 2, "{said}");
        assert!(
            lines[1].starts_with("still on the mark: config.toml"),
            "{said}"
        );
        assert!(lines[1].ends_with("tmux-companion restart"), "{said}");
    }

    #[test]
    fn offline_is_said_in_one_word() {
        let reason = crate::online::reason(100, 280, "1.1.1.1:443");
        assert_eq!(short_word(&reason), "offline");
        let drawn = format_health(&HealthSample {
            reasons: vec![reason],
        });
        assert!(drawn.contains("offline"), "{drawn}");
        assert!(!drawn.contains("1.1.1.1"), "the bar has no room: {drawn}");
    }

    #[test]
    fn quiet_is_the_bell_crossed_out_without_a_word() {
        let drawn = format_health(&HealthSample {
            reasons: vec![crate::quiet::status(Some(1000), None, 700)],
        });
        assert!(drawn.contains(QUIET), "{drawn}");
        assert!(!drawn.contains("quiet"), "{drawn}");
        assert!(!drawn.contains(HEALTH), "{drawn}");
        // After the battery the mark stays in the battery's block: it
        // carries a foreground and leaves the background alone.
        assert!(!drawn.contains("bg="), "{drawn}");
        let two = format_health(&HealthSample {
            reasons: vec![
                crate::quiet::status(Some(1000), None, 700),
                "config.toml changed".into(),
            ],
        });
        assert!(two.contains(&format!("{QUIET} +1")), "{two}");
    }

    #[test]
    fn one_reason_carries_no_count() {
        let sample = HealthSample {
            reasons: reasons(&[], true, false),
        };
        let drawn = format_health(&sample);
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
