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
