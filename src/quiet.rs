//! Quiet hours: nothing nags for a while, and the bar says why.
//!
//! `[notify]` announces finished commands, the inbox nudges about waiting
//! agents, and the `agents` segment counts them; all three hold their tongue
//! while quiet is on. The health mark shows `quiet` instead, so a bar that has
//! gone silent reads as chosen rather than broken. The daemon keeps the clock,
//! so every client and every timer sees the same answer.

/// Seconds from a duration written the way `age` prints one: `45m`, `2h`,
/// `90s`, `1d`, or a bare number of minutes.
pub fn parse_duration(text: &str) -> Option<u64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let (digits, unit) = match t.find(|c: char| !c.is_ascii_digit()) {
        Some(i) => t.split_at(i),
        None => (t, "m"),
    };
    let n: u64 = digits.parse().ok()?;
    let mult = match unit.trim() {
        "s" | "sec" | "secs" => 1,
        "m" | "min" | "mins" => 60,
        "h" | "hr" | "hrs" => 3600,
        "d" | "day" | "days" => 86_400,
        _ => return None,
    };
    n.checked_mul(mult)
}

/// A daily window, `22:00-08:00`, as minutes after midnight. The end may be
/// earlier than the start, which is a window that crosses midnight.
pub fn parse_window(text: &str) -> Option<(u32, u32)> {
    let (start, end) = text.trim().split_once('-')?;
    let minute = |t: &str| -> Option<u32> {
        let (h, m) = t.trim().split_once(':')?;
        let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    };
    let (start, end) = (minute(start)?, minute(end)?);
    (start != end).then_some((start, end))
}

/// Minutes until the window ends, when `minute` of the day is inside it.
pub fn in_window(minute: u32, (start, end): (u32, u32)) -> Option<u32> {
    const DAY: u32 = 24 * 60;
    let inside = if start < end {
        minute >= start && minute < end
    } else {
        minute >= start || minute < end
    };
    inside.then(|| (end + DAY - minute) % DAY)
}

/// Seconds of scheduled quiet left at `minute` of the day: the longest of
/// the windows it is inside, so two that overlap read as one.
pub fn scheduled(windows: &[String], minute: u32) -> Option<u64> {
    windows
        .iter()
        .filter_map(|w| parse_window(w))
        .filter_map(|w| in_window(minute, w))
        .max()
        .map(|m| u64::from(m) * 60)
}

/// Minutes since local midnight, for the daily windows.
pub fn local_minute(secs: u64) -> u32 {
    let t = libc::time_t::try_from(secs).unwrap_or(0);
    // SAFETY: `tm` is a plain C struct that `localtime_r` fills completely,
    // and both pointers are valid for the call.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    u32::try_from(tm.tm_hour * 60 + tm.tm_min).unwrap_or(0)
}

/// When quiet ends at `now`: the later of the timer `quiet` set and the end
/// of a daily window it is inside.
pub fn effective_until(until: Option<u64>, windows: &[String], now: u64) -> Option<u64> {
    let timer = until.filter(|&u| u > now);
    let daily = if windows.is_empty() {
        None
    } else {
        scheduled(windows, local_minute(now)).map(|left| now + left)
    };
    timer.max(daily)
}

/// Whether quiet is on at `now`, given when it ends.
pub fn is_quiet(until: Option<u64>, now: u64) -> bool {
    until.is_some_and(|u| u > now)
}

/// The one line `quiet --status` prints and the health mark reasons from.
pub fn status(until: Option<u64>, now: u64) -> String {
    match until {
        Some(u) if u > now => format!("quiet for {} more", crate::panes::age(u - now)),
        _ => "not quiet".to_string(),
    }
}

/// `quiet`: turn it on for a while, off, or ask.
pub async fn run(duration: Option<String>, off: bool) -> anyhow::Result<()> {
    // `off` as the duration is what a prompt in tmux.conf sends, since one
    // prompt cannot also carry a flag.
    let off = off || duration.as_deref().is_some_and(|d| d.trim() == "off");
    let duration = duration.filter(|d| d.trim() != "off");
    let secs = match (&duration, off) {
        (Some(_), true) => anyhow::bail!("--off takes no duration"),
        (Some(d), false) => Some(
            parse_duration(d)
                .ok_or_else(|| anyhow::anyhow!("not a duration: {d} (try 45m, 2h)"))?,
        ),
        (None, true) => Some(0),
        (None, false) => None,
    };
    let args = crate::proto::QuietArgs { secs };
    let resp = crate::client::send(crate::proto::Request::build("__quiet", &args)).await?;
    if let Some(why) = resp.error {
        anyhow::bail!("{why}");
    }
    println!("{}", resp.output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_two_clock_times_and_may_cross_midnight() {
        assert_eq!(parse_window("22:00-08:00"), Some((1320, 480)));
        assert_eq!(parse_window(" 13:30 - 14:00 "), Some((810, 840)));
        assert_eq!(parse_window("25:00-08:00"), None);
        assert_eq!(parse_window("9-17"), None);
        assert_eq!(parse_window("10:00-10:00"), None);
    }

    #[test]
    fn inside_a_window_says_how_long_is_left() {
        let night = (1320, 480);
        assert_eq!(in_window(23 * 60, night), Some(9 * 60));
        assert_eq!(in_window(7 * 60, night), Some(60));
        assert_eq!(in_window(12 * 60, night), None);
        assert_eq!(in_window(480, night), None, "the end is not inside");
        assert_eq!(in_window(13 * 60 + 45, (810, 840)), Some(15));
    }

    #[test]
    fn the_longest_overlapping_window_wins_and_a_bad_one_is_skipped() {
        let w = vec![
            "22:00-08:00".to_string(),
            "nonsense".into(),
            "23:00-09:00".into(),
        ];
        assert_eq!(scheduled(&w, 23 * 60 + 30), Some((9 * 60 + 30) * 60));
        assert_eq!(scheduled(&w, 12 * 60), None);
    }

    #[test]
    fn the_timer_and_the_schedule_give_the_later_end() {
        assert_eq!(effective_until(Some(500), &[], 100), Some(500));
        assert_eq!(effective_until(Some(50), &[], 100), None);
        assert_eq!(effective_until(None, &[], 100), None);
    }

    #[test]
    fn durations_read_the_way_ages_print() {
        assert_eq!(parse_duration("45m"), Some(2700));
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("90s"), Some(90));
        assert_eq!(parse_duration("1d"), Some(86_400));
        assert_eq!(parse_duration("30"), Some(1800), "a bare number is minutes");
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("soon"), None);
        assert_eq!(parse_duration("5x"), None);
    }

    #[test]
    fn quiet_ends_when_the_clock_passes_it() {
        assert!(is_quiet(Some(1000), 999));
        assert!(!is_quiet(Some(1000), 1000));
        assert!(!is_quiet(None, 0));
        assert_eq!(status(Some(1000), 700), "quiet for 5m more");
        assert_eq!(status(Some(1000), 1000), "not quiet");
        assert_eq!(status(None, 5), "not quiet");
    }
}
