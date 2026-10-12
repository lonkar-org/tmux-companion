//! Quiet hours: nothing nags for a while, and the bar says why.
//!
//! `[notify]` announces finished commands, the inbox nudges about waiting
//! agents, and the `agents` segment counts them; all three hold their tongue
//! while quiet is on. The health mark shows a crossed-out bell instead, so a bar
//! that has gone silent reads as chosen rather than broken. The daemon keeps the clock,
//! so every client and every timer sees the same answer.
//!
//! Quiet also follows a Focus, the operating system's own "don't bother me":
//! `quiet --focus NAME` and `--focus-off` say one started and ended, for a
//! macOS Shortcuts automation or any other hook to call, and with
//! `[quiet] follow_focus` on macOS the daemon reads the Focus database itself;
//! on Linux `dnd` follows the desktop's Do Not Disturb.
//! See `docs/dev/design-quiet-focus.md`.

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

/// The one line `quiet` prints and the health mark reasons from: the Focus
/// first when one is on, since it has no end anybody can count down to.
pub fn status(until: Option<u64>, focus: Option<&str>, now: u64) -> String {
    let left = until
        .filter(|&u| u > now)
        .map(|u| crate::panes::age(u - now));
    // `Focus` alone is a hook that did not say which: "a Focus", not
    // "Focus Focus". Do Not Disturb is said by its name alone, on both
    // systems: "Focus Do Not Disturb" reads badly aloud.
    let focus = focus.map(|f| {
        if f == UNNAMED_FOCUS {
            "a Focus".to_string()
        } else if f == DO_NOT_DISTURB {
            f.to_string()
        } else {
            format!("Focus {f}")
        }
    });
    match (focus, left) {
        (Some(f), Some(left)) => format!("quiet while {f} is on, and for {left} more"),
        (Some(f), None) => format!("quiet while {f} is on"),
        (None, Some(left)) => format!("quiet for {left} more"),
        (None, None) => "not quiet".to_string(),
    }
}

/// Whether a line is one `status` wrote for quiet being on, for the readers
/// that pick it out of the health reasons.
pub fn is_status_line(line: &str) -> bool {
    line.starts_with("quiet for ") || line.starts_with("quiet while ")
}

// ── Focus ───────────────────────────────────────────────────────────────────

/// What a Focus is called when nothing names it: `quiet --focus` with no name,
/// or a mode the configurations file does not list.
pub const UNNAMED_FOCUS: &str = "Focus";

/// The name macOS gives its Do Not Disturb Focus, and the one a Linux
/// desktop's do-not-disturb is reported under: said without the word Focus.
pub const DO_NOT_DISTURB: &str = "Do Not Disturb";

/// The Focus that is on, from the text of `Assertions.json` and
/// `ModeConfigurations.json`, or none.
///
/// An assertion is macOS recording that a mode was turned on, by hand, by a
/// schedule or by a smart trigger, and it leaves the list when the mode ends.
/// When there are several the newest wins. The name comes from the mode's
/// configuration; a mode it does not list is called by the last part of its
/// identifier, and a configurations file that does not parse costs the name,
/// not the answer.
///
/// The format is Apple's and undocumented, so anything that is not the shape
/// below is an error rather than a guess: the caller says so on the health
/// mark and quiet falls back to the timer and the schedule.
pub fn focus_from(assertions: &str, configurations: &str) -> Result<Option<String>, String> {
    let doc: serde_json::Value =
        serde_json::from_str(assertions).map_err(|e| format!("Assertions.json: {e}"))?;
    let data = doc
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("Assertions.json has no `data` list")?;
    let newest = data
        .iter()
        .filter_map(|d| d.get("storeAssertionRecords")?.as_array())
        .flatten()
        .filter_map(|r| {
            let id = r
                .get("assertionDetails")?
                .get("assertionDetailsModeIdentifier")?
                .as_str()?;
            let at = r
                .get("assertionStartDateTimestamp")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);
            Some((at, id))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let Some((_, id)) = newest else {
        return Ok(None);
    };
    Ok(Some(
        mode_name(configurations, id).unwrap_or_else(|| fallback_name(id)),
    ))
}

/// The name a mode identifier has in `ModeConfigurations.json`.
fn mode_name(configurations: &str, id: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_str(configurations).ok()?;
    doc.get("data")?.as_array()?.iter().find_map(|d| {
        let name = d
            .get("modeConfigurations")?
            .get(id)?
            .get("mode")?
            .get("name")?
            .as_str()?
            .trim();
        (!name.is_empty()).then(|| name.to_string())
    })
}

/// `com.apple.focus.work` as `work`, for a mode the configurations do not name.
fn fallback_name(id: &str) -> String {
    match id.rsplit('.').next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => UNNAMED_FOCUS.to_string(),
    }
}

/// Where macOS keeps the Focus database, under `home`.
pub fn focus_dir(home: &std::path::Path) -> std::path::PathBuf {
    home.join("Library/DoNotDisturb/DB")
}

/// What one look at the database found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    /// It read, and this Focus is on, or none is.
    Read(Option<String>),
    /// There is no database: a Mac where Focus was never used, or not a Mac.
    Absent,
    /// It is there and could not be read or made sense of, and why.
    Failed(String),
}

/// Read the database in `dir` once. Two small files, read only when the
/// watcher saw them change.
pub fn look(dir: &std::path::Path) -> Look {
    let assertions = match std::fs::read_to_string(dir.join("Assertions.json")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Look::Absent,
        Err(e) => return Look::Failed(format!("cannot read the macOS Focus state: {e}")),
    };
    // Missing or unreadable costs the name only, which `focus_from` covers.
    let configurations =
        std::fs::read_to_string(dir.join("ModeConfigurations.json")).unwrap_or_default();
    match focus_from(&assertions, &configurations) {
        Ok(focus) => Look::Read(focus),
        Err(why) => Look::Failed(format!("cannot make sense of the macOS Focus state: {why}")),
    }
}

/// What the watcher compares between passes: the modification time and size
/// of each file, so two changes inside one second still differ by length.
pub type Stamp = Option<(std::time::SystemTime, u64)>;

/// One `stat`, no read and no process.
pub fn stamp(path: &std::path::Path) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// How often the watcher stats the database. Two seconds is as late as a
/// nudge can be for an agent that stopped as the Focus started, and a `stat`
/// costs microseconds.
pub const FOCUS_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// The health source a failure to read the database is filed under.
pub const FOCUS_SOURCE: &str = "focus";

/// `[quiet] follow_focus`: the daemon's half. A `stat` of the two files on a
/// timer of its own, and a read only when one changed; never a process.
///
/// A database that cannot be read says so on the health mark once, through
/// `note_failure`, and quiet goes on from the timer and `[quiet] daily`; a
/// later change that reads takes the failure back. One that is absent is a
/// Mac that never used Focus, and nothing is said.
pub async fn focus_loop(
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
    dir: std::path::PathBuf,
) {
    let files = [
        dir.join("Assertions.json"),
        dir.join("ModeConfigurations.json"),
    ];
    let mut seen: Option<[Stamp; 2]> = None;
    let mut failing = false;
    loop {
        let now = [stamp(&files[0]), stamp(&files[1])];
        if seen != Some(now) {
            seen = Some(now);
            let found = look(&dir);
            let mut st = state.lock().await;
            match found {
                Look::Read(focus) => {
                    st.focus = focus;
                    st.note_ok(FOCUS_SOURCE);
                    st.health_cache = None;
                    failing = false;
                }
                Look::Absent => {
                    st.focus = None;
                    st.health_cache = None;
                }
                Look::Failed(why) => {
                    if !failing {
                        st.note_failure(
                            FOCUS_SOURCE,
                            format!("focus: {why}; quiet follows the timer and [quiet] daily only"),
                        );
                        st.health_cache = None;
                    }
                    failing = true;
                }
            }
        }
        tokio::time::sleep(FOCUS_POLL).await;
    }
}

/// What `quiet` was asked to do about a Focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusChange {
    /// One started, with its name.
    On(String),
    /// It ended.
    Off,
}

/// `quiet`: turn it on for a while, off, say a Focus started or ended, or ask.
pub async fn run(
    duration: Option<String>,
    off: bool,
    focus: Option<FocusChange>,
) -> anyhow::Result<()> {
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
    let (focus, focus_off) = match focus {
        Some(FocusChange::On(name)) => {
            let name = name.trim();
            let name = if name.is_empty() { UNNAMED_FOCUS } else { name };
            (Some(name.to_string()), false)
        }
        Some(FocusChange::Off) => (None, true),
        None => (None, false),
    };
    let args = crate::proto::QuietArgs {
        secs,
        focus,
        focus_off,
    };
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
        assert_eq!(status(Some(1000), None, 700), "quiet for 5m more");
        assert_eq!(status(Some(1000), None, 1000), "not quiet");
        assert_eq!(status(None, None, 5), "not quiet");
    }

    #[test]
    fn a_focus_is_named_on_the_line_and_the_timer_follows_it() {
        assert_eq!(
            status(None, Some("Work"), 5),
            "quiet while Focus Work is on"
        );
        assert_eq!(
            status(Some(1000), Some("Sleep"), 700),
            "quiet while Focus Sleep is on, and for 5m more"
        );
        assert_eq!(
            status(Some(10), Some("Work"), 700),
            "quiet while Focus Work is on",
            "a timer that ran out says nothing"
        );
        assert_eq!(
            status(None, Some(UNNAMED_FOCUS), 5),
            "quiet while a Focus is on"
        );
        assert_eq!(
            status(None, Some(DO_NOT_DISTURB), 5),
            "quiet while Do Not Disturb is on",
            "Do Not Disturb without the word Focus"
        );
        assert_eq!(
            status(Some(1000), Some(DO_NOT_DISTURB), 700),
            "quiet while Do Not Disturb is on, and for 5m more"
        );
        for line in [status(None, Some("Work"), 5), status(Some(1000), None, 700)] {
            assert!(is_status_line(&line), "{line}");
        }
        assert!(!is_status_line("not quiet"));
        assert!(!is_status_line("offline for 3m"));
    }

    // Built from the shape macOS writes, with made-up identifiers and times:
    // nobody's real database belongs in the repository.
    const CONFIGS: &str = r#"{"data":[{"modeConfigurations":{
        "com.apple.focus.work":{"mode":{"name":"Work","modeIdentifier":"com.apple.focus.work"}},
        "com.apple.sleep.sleep-mode":{"mode":{"name":"Sleep"}},
        "com.example.custom":{"mode":{"name":"  "}}
    }}],"header":{"version":8}}"#;

    fn assertions(records: &str) -> String {
        format!(
            r#"{{"data":[{{"storeAssertionRecords":[{records}],"storeInvalidationRecords":[]}}],"header":{{"version":8,"timestamp":800000000.5}}}}"#
        )
    }

    fn record(id: &str, at: f64) -> String {
        format!(
            r#"{{"assertionUUID":"00000000-0000-0000-0000-000000000000","assertionStartDateTimestamp":{at},"assertionSource":{{"assertionClientIdentifier":"com.example.test"}},"assertionDetails":{{"assertionDetailsIdentifier":"x","assertionDetailsModeIdentifier":"{id}"}}}}"#
        )
    }

    #[test]
    fn no_assertion_is_no_focus() {
        assert_eq!(focus_from(&assertions(""), CONFIGS), Ok(None));
        assert_eq!(
            focus_from(r#"{"data":[{}],"header":{}}"#, CONFIGS),
            Ok(None),
            "a store with no records list yet"
        );
    }

    #[test]
    fn an_assertion_is_named_from_the_configurations() {
        let a = assertions(&record("com.apple.focus.work", 800000000.0));
        assert_eq!(focus_from(&a, CONFIGS), Ok(Some("Work".into())));
    }

    #[test]
    fn the_newest_of_several_assertions_wins() {
        let a = assertions(&format!(
            "{},{}",
            record("com.apple.sleep.sleep-mode", 800000100.0),
            record("com.apple.focus.work", 800000000.0)
        ));
        assert_eq!(focus_from(&a, CONFIGS), Ok(Some("Sleep".into())));
    }

    #[test]
    fn a_mode_with_no_name_is_called_by_its_identifier() {
        let a = assertions(&record("com.example.custom", 1.0));
        assert_eq!(focus_from(&a, CONFIGS), Ok(Some("custom".into())));
        let a = assertions(&record("com.apple.focus.work", 1.0));
        assert_eq!(
            focus_from(&a, "not json"),
            Ok(Some("work".into())),
            "configurations that do not parse cost the name only"
        );
    }

    #[test]
    fn a_database_that_is_not_the_shape_is_an_error_not_a_guess() {
        assert!(focus_from("", CONFIGS).is_err());
        assert!(focus_from(r#"{"header":{}}"#, CONFIGS).is_err());
        assert!(focus_from(r#"{"data":{}}"#, CONFIGS).is_err());
    }

    #[test]
    fn the_database_on_disk_reads_absent_read_and_failed() {
        let dir = std::env::temp_dir().join(format!("tc-focus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(look(&dir), Look::Absent);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Assertions.json"), assertions("")).unwrap();
        assert_eq!(look(&dir), Look::Read(None), "no configurations is fine");
        std::fs::write(
            dir.join("Assertions.json"),
            assertions(&record("com.apple.focus.work", 1.0)),
        )
        .unwrap();
        std::fs::write(dir.join("ModeConfigurations.json"), CONFIGS).unwrap();
        assert_eq!(look(&dir), Look::Read(Some("Work".into())));
        std::fs::write(dir.join("Assertions.json"), "{").unwrap();
        assert!(matches!(look(&dir), Look::Failed(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
