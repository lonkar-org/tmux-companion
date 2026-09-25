//! The daemon's half: a snapshot on a timer, and a marker saying it is alive.
//!
//! The marker is what tells a restore whether the last run ended badly. A
//! snapshot's own `clean` flag cannot answer that: a timer's capture writes
//! `false` because the daemon does not know yet, and nothing goes back to
//! correct the one that turned out to be the last before a crash.
//!
//! The scheduling arithmetic is pure and takes its clock, so a cron expression
//! can be tested at four in the morning on a Sunday without waiting for one.

use std::path::{Path, PathBuf};

use crate::config::{Sessions, SessionsAutosave};

/// The file that exists while a daemon is running.
///
/// Beside the snapshots rather than next to the socket, because it is read by
/// a restore rather than by a client looking for a daemon, and because a
/// socket file is removed by the thing that owns it either way.
pub fn marker_path_in(state_dir: &Path) -> PathBuf {
    super::store::dir_in(state_dir).join("running")
}

/// Record that a daemon is running, so a crash can be told from a clean stop.
pub fn mark_running_in(state_dir: &Path, pid: u32) -> std::io::Result<()> {
    let dir = super::store::dir_in(state_dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(marker_path_in(state_dir), format!("{pid}\n"))
}

/// Remove the marker, which is what makes the next start a clean one.
pub fn clear_marker_in(state_dir: &Path) {
    let _ = std::fs::remove_file(marker_path_in(state_dir));
}

/// Whether a marker was left behind by a daemon that never removed it.
///
/// Its own pid is ignored on purpose. A daemon that is running right now left
/// this file, and asking "did the last one crash" while one is alive is a
/// question about a file, not about a process.
pub fn crashed_in(state_dir: &Path) -> bool {
    marker_path_in(state_dir).exists()
}

/// A moment, in the fields a cron expression is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// 0 to 59.
    pub minute: u32,
    /// 0 to 23.
    pub hour: u32,
    /// 1 to 31.
    pub day: u32,
    /// 1 to 12.
    pub month: u32,
    /// 0 is Sunday, the way cron counts.
    pub weekday: u32,
}

/// Break a unix timestamp into the fields cron cares about, in UTC.
///
/// UTC, like the snapshot stamps, so a schedule does not move twice a year and
/// a machine that sleeps through the change wakes up agreeing with itself.
pub fn moment_from(secs: i64) -> Moment {
    let text = crate::tasks::format_unix(secs);
    let digits: Vec<u32> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    // 1970-01-01 was a Thursday, which cron numbers 4.
    let weekday = (secs.div_euclid(86_400) + 4).rem_euclid(7) as u32;
    Moment {
        minute: digits.get(4).copied().unwrap_or(0),
        hour: digits.get(3).copied().unwrap_or(0),
        day: digits.get(2).copied().unwrap_or(1),
        month: digits.get(1).copied().unwrap_or(1),
        weekday,
    }
}

/// Whether one cron field matches one value.
///
/// Understands `*`, a number, a comma list, a range and a step on any of them,
/// which is the part of cron people write. Names for months and weekdays are
/// not understood, and a field this cannot read matches nothing, so a typo
/// stops the timer rather than running it at a time nobody asked for.
pub fn field_matches(field: &str, value: u32) -> bool {
    field.split(',').any(|part| {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().unwrap_or(0)),
            None => (part, 1),
        };
        if step == 0 {
            return false;
        }
        let (from, to) = if range == "*" {
            (u32::MIN, u32::MAX)
        } else if let Some((a, b)) = range.split_once('-') {
            match (a.parse::<u32>(), b.parse::<u32>()) {
                (Ok(a), Ok(b)) => (a, b),
                _ => return false,
            }
        } else {
            match range.parse::<u32>() {
                Ok(n) => (n, n),
                Err(_) => return false,
            }
        };
        if value < from || value > to {
            return false;
        }
        // A step counts from the start of the range, and from zero when the
        // range is the whole field, which is what `*/15` means.
        let base = if range == "*" { 0 } else { from };
        (value - base) % step == 0
    })
}

/// Whether a five-field cron expression matches a moment.
///
/// Day-of-month and day-of-week are ANDed, not ORed. Real cron ORs them when
/// both are restricted, which is a rule almost nobody knows and which surprises
/// everybody who meets it. This is a snapshot timer, and the surprising reading
/// is not worth inheriting.
pub fn cron_matches(spec: &str, at: Moment) -> bool {
    let fields: Vec<&str> = spec.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    field_matches(fields[0], at.minute)
        && field_matches(fields[1], at.hour)
        && field_matches(fields[2], at.day)
        && field_matches(fields[3], at.month)
        && field_matches(fields[4], at.weekday)
}

/// Whether a snapshot is due, given when the last one was taken.
///
/// `last` is `None` before the first, which is due immediately under
/// `interval` and waits for its slot under `cron`. A daemon started at 09:07
/// with `0 * * * *` takes its first snapshot at 10:00, which is what somebody
/// asking for the top of the hour asked for.
pub fn due(config: &Sessions, last: Option<i64>, now: i64) -> bool {
    match config.autosave {
        SessionsAutosave::Off => false,
        SessionsAutosave::Interval => match last {
            None => true,
            Some(last) => now.saturating_sub(last) >= config.interval_secs as i64,
        },
        SessionsAutosave::Cron => {
            // Once per minute at most, so a tick every few seconds inside the
            // matching minute does not take sixty snapshots of it.
            if last.is_some_and(|last| now.saturating_sub(last) < 60) {
                return false;
            }
            cron_matches(&config.cron, moment_from(now))
        }
    }
}

/// How often the loop wakes to ask whether a snapshot is due.
///
/// Seconds rather than the interval itself, because a cron expression has to be
/// checked inside every minute it might match, and because a loop that sleeps
/// for the whole interval oversleeps by whatever the machine was suspended for.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(20);

/// Run tmux and take its output, or nothing when it could not run.
///
/// Each daemon task makes its own tmux calls, which is how `[notify]` and
/// `[window_names]` already work.
async fn tmux_out(args: &[&str]) -> String {
    match tokio::process::Command::new("tmux")
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .await
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// Everything one snapshot produced, for a caller that wants to report it.
pub struct Taken {
    /// What the capture read, including what it was unsure of.
    pub captured: super::capture::Captured,
    /// Where the snapshot was written.
    pub file: PathBuf,
    /// Its stamp.
    pub stamp: String,
    /// How many panes' screens were captured.
    pub history_panes: usize,
}

/// Capture the server and write it as a new generation.
///
/// One implementation, called by `sessions save` and by the timer, so the
/// snapshot a daemon writes on its own is the same one a person gets by asking.
pub async fn take_snapshot(
    config: &Sessions,
    extra_exclude: &[String],
    skip_history: bool,
    clean: bool,
) -> anyhow::Result<Taken> {
    let state_dir = crate::server::state_dir()
        .ok_or_else(|| anyhow::anyhow!("no state directory: neither XDG_STATE_HOME nor HOME"))?;

    let mut exclude = config.exclude.clone();
    exclude.extend(extra_exclude.iter().cloned());

    let sessions = tmux_out(&["list-sessions", "-F", super::capture::SESSION_FORMAT]).await;
    let windows = tmux_out(&["list-windows", "-a", "-F", super::capture::WINDOW_FORMAT]).await;
    let panes = tmux_out(&["list-panes", "-a", "-F", super::capture::PANE_FORMAT]).await;
    let default_command = tmux_out(&["show-options", "-gv", "default-command"])
        .await
        .trim()
        .to_string();
    let tmux_version = tmux_out(&["-V"]).await.trim().to_string();
    let processes = match tokio::process::Command::new("ps")
        .args(super::capture::PS_ARGS)
        .output()
        .await
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    };

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let at = crate::tasks::format_unix(now);
    let hostname = tmux_out(&["display-message", "-p", "#{host}"]).await;

    let captured = super::capture::capture(&super::capture::Capture {
        sessions: &sessions,
        windows: &windows,
        panes: &panes,
        processes: &processes,
        shell: &shell,
        default_command: &default_command,
        exclude: &exclude,
        at: &at,
        tmux_version: &tmux_version,
        companion_version: &crate::proto::build_id(),
        hostname: hostname.trim(),
        clean,
    });
    if let Some(why) = captured.refusal() {
        anyhow::bail!("{why}");
    }

    let stamp = super::store::stamp_from(now);
    let mut history_panes = 0usize;
    if config.pane_history && !skip_history {
        let lines = format!("-{}", config.pane_history_lines);
        for session in &captured.snapshot.session {
            for window in &session.window {
                for pane in &window.pane {
                    let target = format!("={}:{}.{}", session.name, window.index, pane.index);
                    let text = tmux_out(&["capture-pane", "-p", "-S", &lines, "-t", &target]).await;
                    if super::store::store_history_in(
                        &state_dir,
                        &stamp,
                        &session.name,
                        window.index,
                        pane.index,
                        &text,
                    )
                    .is_ok()
                    {
                        history_panes += 1;
                    }
                }
            }
        }
    }

    let file = super::store::store_in(
        &state_dir,
        &captured.snapshot,
        &stamp,
        config.keep,
        config.keep_days,
        now,
    )?;

    Ok(Taken {
        captured,
        file,
        stamp,
        history_panes,
    })
}

/// Take a snapshot on the schedule the config asks for, until the daemon stops.
///
/// It wakes every [`TICK`] and asks whether one is due rather than sleeping for
/// the interval, because a cron slot has to be noticed inside the minute it
/// names and because a long sleep oversleeps by however long the machine was
/// suspended.
pub async fn sessions_autosave_loop(config: Sessions) {
    let mut last: Option<i64> = None;
    loop {
        tokio::time::sleep(TICK).await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if !due(&config, last, now) {
            continue;
        }
        // `clean` is false: this daemon does not know yet whether it will stop
        // politely, and a snapshot that turned out to be the last one before a
        // crash is exactly the one nobody comes back to correct.
        match take_snapshot(&config, &[], false, false).await {
            Ok(_) => last = Some(now),
            // A server that is not running, or one whose output could not be
            // read, is not worth a line in a log nobody reads. The next tick
            // tries again.
            Err(_) => last = Some(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minute: u32, hour: u32, day: u32, month: u32, weekday: u32) -> Moment {
        Moment {
            minute,
            hour,
            day,
            month,
            weekday,
        }
    }

    #[test]
    fn a_marker_outlives_the_daemon_that_could_not_remove_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!crashed_in(dir.path()));

        mark_running_in(dir.path(), 4242).expect("mark");
        // A daemon killed with -9 removes nothing, so the file is still here.
        assert!(crashed_in(dir.path()));

        clear_marker_in(dir.path());
        assert!(!crashed_in(dir.path()));
        // Clearing a marker that is not there is not an error.
        clear_marker_in(dir.path());
    }

    #[test]
    fn the_marker_holds_the_pid_that_wrote_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        mark_running_in(dir.path(), 4242).expect("mark");
        let text = std::fs::read_to_string(marker_path_in(dir.path())).expect("read");
        assert_eq!(text.trim(), "4242");
    }

    #[test]
    fn a_timestamp_becomes_the_fields_cron_matches_on() {
        // 2026-09-25 09:32:11 UTC, a Friday.
        let m = moment_from(1_790_328_731);
        assert_eq!(m.minute, 32);
        assert_eq!(m.hour, 9);
        assert_eq!(m.day, 25);
        assert_eq!(m.month, 9);
        assert_eq!(m.weekday, 5);
    }

    #[test]
    fn the_epoch_was_a_thursday() {
        assert_eq!(moment_from(0).weekday, 4);
        assert_eq!(moment_from(86_400).weekday, 5);
        assert_eq!(moment_from(86_400 * 3).weekday, 0);
    }

    #[test]
    fn a_star_matches_everything_and_a_number_matches_itself() {
        assert!(field_matches("*", 0));
        assert!(field_matches("*", 59));
        assert!(field_matches("7", 7));
        assert!(!field_matches("7", 8));
    }

    #[test]
    fn steps_count_from_zero_on_a_star_and_from_the_start_of_a_range() {
        assert!(field_matches("*/15", 0));
        assert!(field_matches("*/15", 30));
        assert!(!field_matches("*/15", 31));
        assert!(field_matches("10-20/5", 10));
        assert!(field_matches("10-20/5", 15));
        assert!(!field_matches("10-20/5", 16));
        assert!(!field_matches("10-20/5", 25));
    }

    #[test]
    fn a_comma_list_matches_any_of_its_parts() {
        assert!(field_matches("0,15,30,45", 30));
        assert!(!field_matches("0,15,30,45", 31));
    }

    #[test]
    fn a_field_this_cannot_read_matches_nothing() {
        // A typo stops the timer rather than running it at a time nobody asked
        // for, which is the direction to fail in.
        assert!(!field_matches("MON", 1));
        assert!(!field_matches("*/0", 0));
        assert!(!field_matches("5-", 5));
        assert!(!cron_matches("0 *", at(0, 0, 1, 1, 4)));
        assert!(!cron_matches("", at(0, 0, 1, 1, 4)));
    }

    #[test]
    fn the_top_of_every_hour_is_the_shipped_default() {
        assert!(cron_matches("0 * * * *", at(0, 9, 25, 9, 5)));
        assert!(!cron_matches("0 * * * *", at(1, 9, 25, 9, 5)));
    }

    #[test]
    fn day_of_month_and_day_of_week_both_have_to_agree() {
        // Real cron ORs these when both are restricted. This does not, because
        // the OR is a rule almost nobody knows.
        assert!(cron_matches("0 0 25 * 5", at(0, 0, 25, 9, 5)));
        assert!(!cron_matches("0 0 25 * 1", at(0, 0, 25, 9, 5)));
        assert!(!cron_matches("0 0 24 * 5", at(0, 0, 25, 9, 5)));
    }

    fn every(mode: SessionsAutosave, secs: u64) -> Sessions {
        Sessions {
            autosave: mode,
            interval_secs: secs,
            ..Sessions::default()
        }
    }

    #[test]
    fn nothing_is_ever_due_while_the_timer_is_off() {
        let config = every(SessionsAutosave::Off, 10);
        assert!(!due(&config, None, 1_000_000));
        assert!(!due(&config, Some(0), 1_000_000));
    }

    #[test]
    fn an_interval_is_due_immediately_and_then_once_per_interval() {
        let config = every(SessionsAutosave::Interval, 900);
        assert!(due(&config, None, 1_000_000));
        assert!(!due(&config, Some(1_000_000), 1_000_400));
        assert!(due(&config, Some(1_000_000), 1_000_900));
        assert!(due(&config, Some(1_000_000), 1_009_000));
    }

    #[test]
    fn a_cron_schedule_waits_for_its_slot_rather_than_firing_at_startup() {
        let mut config = every(SessionsAutosave::Cron, 900);
        config.cron = "0 * * * *".to_string();
        // 2026-09-25 09:32:11 UTC: not the top of an hour, so not yet.
        assert!(!due(&config, None, 1_790_328_731));
        // 2026-09-25 10:00:00 UTC.
        assert!(due(&config, None, 1_790_330_400));
    }

    #[test]
    fn a_cron_slot_fires_once_rather_than_every_tick_inside_its_minute() {
        let mut config = every(SessionsAutosave::Cron, 900);
        config.cron = "0 * * * *".to_string();
        let slot = 1_790_330_400;
        assert!(due(&config, None, slot));
        // Twenty seconds later, still inside the minute, already taken.
        assert!(!due(&config, Some(slot), slot + 20));
        // The next hour is a new slot.
        assert!(due(&config, Some(slot), slot + 3_600));
    }

    #[test]
    fn the_tick_is_short_enough_to_land_inside_any_cron_minute() {
        assert!(TICK < std::time::Duration::from_secs(60));
    }
}
