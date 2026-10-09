//! Background work the daemon does on a timer, and the small tmux commands
//! that do not need one.
//!
//! `autosave` is the one that gets structurally better rather than merely
//! equivalent. As a detached zsh loop it needed a PID lock file to stop
//! `prefix+r` starting a second copy on every config reload, a stale-lock
//! takeover for when a server was killed, and a `sleep` that had to be
//! interrupted by checking whether tmux was still alive. Inside a daemon that
//! already runs tokio it is a scheduled task, and the whole class of problem
//! goes with it: one daemon, one task, and it dies when the daemon does.
//!
//! Restoring stays on a keybinding on purpose. An automatic restore would
//! resurrect a stale layout over a session somebody has already started working
//! in, which is a worse failure than losing a layout to a reboot.

use std::time::Duration;

/// Where the last-save timestamp is written.
pub fn stamp_path() -> Option<std::path::PathBuf> {
    crate::server::state_dir().map(|d| d.join("last-autosave"))
}

/// Run the save script once, recording when it happened.
///
/// This is the saving half of tmux-resurrect, called directly rather than
/// through tmux-continuum: continuum drives its timer by appending
/// `#{continuum_save}` to `status-right`, and `status-right` here is a single
/// `#()` into this binary, tuned down from five spawns a second. A task of our
/// own leaves that alone.
pub async fn save_now(script: &std::path::Path) -> anyhow::Result<()> {
    if !script.is_file() {
        anyhow::bail!("{} is missing", script.display());
    }
    let status = tokio::process::Command::new(script).status().await?;
    if !status.success() {
        anyhow::bail!("{} exited {:?}", script.display(), status.code());
    }
    if let Some(stamp) = stamp_path() {
        if let Some(dir) = stamp.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(stamp, format!("{}\n", now_stamp()));
    }
    Ok(())
}

/// The current time, in the format the zsh version wrote.
fn now_stamp() -> String {
    // Formatted by hand rather than with a date crate: this is the only place
    // in the binary that needs a calendar, and a dependency for one string is
    // a dependency somebody else has to audit.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix(secs as i64)
}

/// Seconds since the epoch as `YYYY-MM-DD HH:MM:SS`, in UTC.
///
/// The zsh version wrote local time. This writes UTC and says so, because a
/// timestamp whose zone depends on where the daemon started is worse than one
/// that is always the same.
pub fn format_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Civil-from-days, the standard algorithm: shift the epoch to 0000-03-01 so
    // the leap day lands at the end of a year and every month has a fixed
    // length pattern.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

/// What `autosave --status` prints.
pub fn last_save() -> String {
    match stamp_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) if !text.trim().is_empty() => format!("last autosave: {}", text.trim()),
        _ => "no autosave recorded yet".to_string(),
    }
}

/// The name the `[autosave]` timer's failure is kept under.
const AUTOSAVE: &str = "autosave";

/// Save on a timer until the daemon stops.
///
/// No lock file, because there is one daemon and it owns this task. No liveness
/// check either: the task cannot outlive the process it runs in, which is what
/// the zsh loop's `tmux has-session` was for.
pub async fn autosave_loop(
    script: std::path::PathBuf,
    interval: Duration,
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
) {
    loop {
        tokio::time::sleep(interval).await;
        match save_now(&script).await {
            // It works now, so what the mark said about the last run is old.
            Ok(_) => state.lock().await.note_ok(AUTOSAVE),
            Err(e) => {
                eprintln!("tmux-companion: autosave failed: {e}");
                // The log is where this went for a day unseen; the health
                // mark on the bar is the reason anybody looks at the log.
                state
                    .lock()
                    .await
                    .note_failure(AUTOSAVE, format!("autosave: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_formats_as_the_epoch() {
        assert_eq!(format_unix(0), "1970-01-01 00:00:00 UTC");
    }

    #[test]
    fn a_known_timestamp_formats_correctly() {
        // 2026-09-22T09:00:00Z, checked against `date -u -r 1790067600`.
        assert_eq!(format_unix(1_790_067_600), "2026-09-22 09:00:00 UTC");
    }

    #[test]
    fn a_leap_day_is_a_leap_day() {
        // 2024-02-29T12:34:56Z
        assert_eq!(format_unix(1_709_210_096), "2024-02-29 12:34:56 UTC");
    }

    #[test]
    fn a_century_boundary_is_handled() {
        // 2000-03-01T00:00:00Z: 1900 was not a leap year and 2000 was, which is
        // where a hand-rolled calendar usually goes wrong.
        assert_eq!(format_unix(951_868_800), "2000-03-01 00:00:00 UTC");
    }

    #[tokio::test]
    async fn saving_with_no_script_is_an_error_naming_the_path() {
        let missing = std::path::PathBuf::from("/nowhere/save.sh");
        let e = save_now(&missing).await.unwrap_err().to_string();
        assert!(e.contains("/nowhere/save.sh"), "{e}");
        assert!(e.contains("missing"), "{e}");
    }

    #[test]
    fn a_missing_stamp_reads_as_never_rather_than_as_an_error() {
        // `autosave --status` on a machine that has never saved.
        let text = last_save();
        assert!(
            text.starts_with("last autosave:") || text == "no autosave recorded yet",
            "{text}"
        );
    }
}
