//! Sessions nobody has looked at for days: which they are, and the picker
//! that closes one.
//!
//! A session per project is cheap to open and easy to forget. After a few
//! weeks the project picker's live half is a list of everything that was ever
//! started, and the sessions worth keeping are buried among the ones nobody
//! has been in since. tmux knows two things that tell them apart: whether a
//! client is attached, and when the session last had any activity. Idle is
//! detached and quiet for longer than a threshold, and the answer is a list to
//! close from, one at a time, through [`crate::close`] so the layout is kept
//! on the way out.
//!
//! The parse and the age arithmetic take the listing and the clock as
//! arguments, so they are tested against a fixed `now` with no server in the
//! room. Only [`list`] and [`run`] talk to tmux and the terminal.

use crate::project::short_path;

/// The `list-sessions -F` format [`parse`] reads: name, path, attached
/// clients, last activity in unix seconds, and the window count.
pub const FORMAT: &str = "#{session_name}\t#{session_path}\t#{session_attached}\t#{session_activity}\t#{session_windows}";

/// Seconds in a day, which is the unit `--days` is counted in.
pub const DAY: u64 = 86_400;

/// One session with nobody in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleSession {
    /// The session's name.
    pub name: String,
    /// The session's directory.
    pub path: String,
    /// How many windows it holds.
    pub windows: u64,
    /// Seconds since anything happened in it.
    pub idle: u64,
}

/// How long a detached session has been quiet, or `None` when a client is on
/// it.
///
/// An attached session is somebody's, however still it is: a shell left at a
/// prompt in an attached client is where they are working, not something to
/// close. A clock behind the activity stamp answers zero rather than wrapping.
pub fn idle_for(attached: u64, activity: u64, now: u64) -> Option<u64> {
    (attached == 0).then(|| now.saturating_sub(activity))
}

/// An age as a person reads it: `12s`, `3m`, `2h`, `5d`.
///
/// One unit, the largest that fits, rounded down. Nobody deciding whether to
/// close a session needs `5d 3h`, and the column stays narrow.
pub fn age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m", secs / 60)
    } else if secs < DAY {
        format!("{}h", secs / 3_600)
    } else {
        format!("{}d", secs / DAY)
    }
}

/// The sessions in a [`FORMAT`] listing that are detached and have been quiet
/// for longer than `min_idle` seconds, most idle first.
///
/// A line that does not parse is skipped rather than failing the list: a
/// session name can hold anything but a dot or a colon, and a tab in one
/// would shift the fields, but nothing here is written to disk on the
/// strength of the answer, so a row wrong is a row missing from a picker.
pub fn parse(listing: &str, now: u64, min_idle: u64) -> Vec<IdleSession> {
    let mut out: Vec<IdleSession> = listing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut f = line.split('\t');
            let name = f.next()?.to_string();
            let path = f.next()?.to_string();
            let attached: u64 = f.next()?.trim().parse().ok()?;
            let activity: u64 = f.next()?.trim().parse().ok()?;
            let windows: u64 = f.next()?.trim().parse().ok()?;
            let idle = idle_for(attached, activity, now)?;
            (idle > min_idle).then_some(IdleSession {
                name,
                path,
                windows,
                idle,
            })
        })
        .collect();
    // Most idle first, so the one that can go soonest is the first row; the
    // question this list answers is "which", not "where is X".
    out.sort_by_key(|s| std::cmp::Reverse(s.idle));
    out
}

/// The picker's columns for one session, which `--print` writes as a TSV row.
pub fn columns(s: &IdleSession, home: &str) -> Vec<String> {
    vec![
        s.name.clone(),
        short_path(&s.path, home),
        format!("idle {}", age(s.idle)),
        s.windows.to_string(),
    ]
}

/// Now, in unix seconds, which is what `#{session_activity}` is measured in.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Ask tmux, and keep the sessions idle for more than `days`.
pub async fn list(days: u64) -> Vec<IdleSession> {
    let listing = crate::cli::tmux_capture(&["list-sessions", "-F", FORMAT]).await;
    parse(&listing, now(), days.saturating_mul(DAY))
}

/// `sessions idle`: list, or open the picker, and say which session was
/// picked.
///
/// The close itself is left to the caller. It is `project close` by another
/// route, and that command lives in [`crate::cli`] beside the rest of the
/// project commands; this only answers which session.
pub async fn run(days: u64, print: bool) -> anyhow::Result<Option<String>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let sessions = list(days).await;
    if sessions.is_empty() {
        // Not an error: the usual answer on a tidy server, and a script
        // calling this in a loop wants a zero, not a stack of failures.
        eprintln!(
            "no session idle for {days} day{}",
            crate::cli::plural(days as usize)
        );
        return Ok(None);
    }

    if print {
        for s in &sessions {
            println!("{}", columns(s, &home).join("\t"));
        }
        return Ok(None);
    }

    let items: Vec<crate::picker::Item> = sessions
        .iter()
        .map(|s| {
            let cols = columns(s, &home);
            crate::picker::Item::new(cols.join(" ")).in_columns(cols)
        })
        .collect();

    let config = crate::cli::config_or_default();
    // The project picker's shape from [picker]: this lists sessions the same
    // way that one does, and a [picker.idle] of its own would be a section
    // nobody has asked to set differently.
    let chrome = crate::picker::Chrome {
        title: "[ Idle sessions ]".into(),
        footer: "enter closes the pick (project close, layout saved first)   esc cancels".into(),
        ..Default::default()
    }
    .laid_out_by(&config.picker.resolved(crate::config::Picker::Project));

    let Some(index) = crate::picker::run(items, "", &chrome)? else {
        return Ok(None);
    };
    Ok(sessions.get(index).map(|s| s.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000;

    #[test]
    fn an_age_is_one_unit_rounded_down() {
        assert_eq!(age(0), "0s");
        assert_eq!(age(12), "12s");
        assert_eq!(age(59), "59s");
        assert_eq!(age(60), "1m");
        assert_eq!(age(3 * 60 + 59), "3m");
        assert_eq!(age(2 * 3_600 + 3_599), "2h");
        assert_eq!(age(DAY - 1), "23h");
        assert_eq!(age(DAY), "1d");
        assert_eq!(age(5 * DAY + 3_600), "5d");
    }

    #[test]
    fn an_attached_session_is_never_idle_and_a_detached_one_is_aged() {
        assert_eq!(idle_for(1, NOW - 10 * DAY, NOW), None);
        assert_eq!(idle_for(0, NOW - 10 * DAY, NOW), Some(10 * DAY));
        // A clock behind the stamp is zero, not a wrap to centuries.
        assert_eq!(idle_for(0, NOW + 5, NOW), Some(0));
    }

    #[test]
    fn only_detached_sessions_older_than_the_threshold_come_back_most_idle_first() {
        let listing = format!(
            "fresh\t/f\t0\t{}\t2\nold\t/o\t0\t{}\t1\nolder\t/oo\t0\t{}\t4\nheld\t/h\t1\t{}\t3\n",
            NOW - 2 * DAY,
            NOW - 4 * DAY,
            NOW - 9 * DAY,
            NOW - 30 * DAY,
        );
        let rows = parse(&listing, NOW, 3 * DAY);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        // `fresh` is under the threshold and `held` has a client on it.
        assert_eq!(names, vec!["older", "old"]);
        assert_eq!(rows[0].windows, 4);
        assert_eq!(rows[0].idle, 9 * DAY);
    }

    #[test]
    fn exactly_the_threshold_is_not_older_than_it() {
        let listing = format!("edge\t/e\t0\t{}\t1\n", NOW - 3 * DAY);
        assert!(parse(&listing, NOW, 3 * DAY).is_empty());
        assert_eq!(parse(&listing, NOW, 3 * DAY - 1).len(), 1);
    }

    #[test]
    fn a_line_that_does_not_parse_is_skipped_not_fatal() {
        let listing = format!(
            "good\t/g\t0\t{}\t1\nbroken\t/b\tx\ty\tz\nshort\n",
            NOW - 5 * DAY
        );
        let rows = parse(&listing, NOW, DAY);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "good");
    }

    #[test]
    fn the_columns_are_name_short_path_age_and_windows() {
        let s = IdleSession {
            name: "mysetup".into(),
            path: "/Users/yogesh/git-repos/mysetup".into(),
            windows: 2,
            idle: 5 * DAY,
        };
        assert_eq!(
            columns(&s, "/Users/yogesh"),
            vec!["mysetup", "~/g/mysetup", "idle 5d", "2"]
        );
    }
}
