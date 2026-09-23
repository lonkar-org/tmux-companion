//! Network bandwidth: a counter read, the arithmetic that turns two reads
//! into a rate, and the IEC formatting that keeps the segment narrow.
use std::time::{Duration, Instant};

use crate::tmux::icons::{ARROW_LEFT, RATE_GIB, RATE_KIB, RATE_MIB};

/// The default the config starts from, and what the tests measure against.
///
/// `[network] threshold_bps` is what decides, and used not to: the renderer
/// read this constant directly and the setting did nothing at all. Kept as the
/// one definition of the default, which `config::Network` takes.
#[cfg(test)]
const THRESHOLD_BPS: u64 = 20_480; // 20 KiB/s default threshold

/// Below this interval a delta is not divided: a handful of bytes over a few
/// milliseconds extrapolates to a nonsense rate, and two clients refreshing
/// back to back would make the bar flicker between a real number and a spike.
pub const MIN_ELAPSED: Duration = Duration::from_millis(200);

/// A cumulative counter reading and the instant it was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetSample {
    /// Cumulative bytes received.
    pub rx: u64,
    /// Cumulative bytes sent.
    pub tx: u64,
    /// When the counters were read.
    pub at: Instant,
}

/// Format a byte rate. Each multiple-of-1024 unit is a single glyph rather than
/// spelled-out text, so the segment stays narrow; plain bytes keep their text
/// form.
fn iec_fmt(bytes_per_sec: u64, pad: usize) -> String {
    const K: u64 = 1024;
    let (val, unit) = if bytes_per_sec >= K * K * K {
        (bytes_per_sec / (K * K * K), RATE_GIB)
    } else if bytes_per_sec >= K * K {
        (bytes_per_sec / (K * K), RATE_MIB)
    } else if bytes_per_sec >= K {
        (bytes_per_sec / K, RATE_KIB)
    } else {
        (bytes_per_sec, "B/s")
    };
    format!("{:>pad$}{}", val, unit)
}

/// Style the unit part of a speed string: `20KiB/s` → `20#[fg=colour237,none,italics]KiB/s#[none]`.
/// Equivalent to: sed -E 's/([0-9]+)(.+)/\1#[fg=colour237,none,italics]\2#[none]/g'
fn iec_fmt_styled(bytes_per_sec: u64, unit_colour: &str) -> String {
    let plain = iec_fmt(bytes_per_sec, 0);
    let split = plain
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(plain.len());
    let (num, unit) = plain.split_at(split);
    format!("{num}#[fg={unit_colour},none,italics]{unit}#[none]")
}

/// Format bandwidth delta into tmux status segment(s).
/// Returns empty string when both are below threshold.
///
/// The colours are settings rather than constants for the same reason the
/// threshold is: the text is drawn in the bar's background colour on top of
/// the rate block, so a bar that is not `colour233` had this segment writing
/// in a colour from somebody else's tmux.conf.
pub fn format_rates(dl: u64, ul: u64, net: &crate::config::Network, bar_bg: &str) -> String {
    let mut out = String::new();
    if dl >= net.threshold_bps {
        out.push_str(&format!(
            "#[fg={0}]{1}#[fg={2},bg={0}]{3}",
            net.download_colour,
            ARROW_LEFT,
            bar_bg,
            iec_fmt_styled(dl, &net.unit_colour)
        ));
    }
    if ul >= net.threshold_bps {
        out.push_str(&format!(
            "#[fg={0}]{1}#[fg={2},bg={0}]{3}",
            net.upload_colour,
            ARROW_LEFT,
            bar_bg,
            iec_fmt_styled(ul, &net.unit_colour)
        ));
    }
    out
}

/// Read cumulative rx/tx bytes from all non-loopback interfaces via sysinfo.
/// Replaces the `netstat -ibn` subprocess; eliminates the periodic netstat stall.
fn read_net_bytes_native() -> (u64, u64) {
    let networks = sysinfo::Networks::new_with_refreshed_list();
    networks
        .iter()
        .filter(|(name, _)| !name.starts_with("lo"))
        .fold((0u64, 0u64), |(rx, tx), (_, data)| {
            (rx + data.total_received(), tx + data.total_transmitted())
        })
}

/// Take a counter reading off the async runtime.
///
/// This is the only expensive half of the segment and it touches no shared
/// state, so the combined status side can run it concurrently with the other
/// segments and apply the arithmetic afterwards under one short lock.
pub async fn sample() -> anyhow::Result<(u64, u64)> {
    Ok(tokio::task::spawn_blocking(read_net_bytes_native).await?)
}

/// Bytes per second over `elapsed`, as a whole number.
///
/// `as_secs_f64` rather than `as_secs`: truncating to whole seconds reported a
/// 1.1-second interval as one second and inflated every rate by the remainder —
/// about 10% at the intervals tmux actually produces.
pub fn rate(delta: u64, elapsed: Duration) -> u64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return 0;
    }
    (delta as f64 / secs) as u64
}

/// Fold a new counter reading into the running state and return what to draw.
///
/// Pure apart from its two `&mut` arguments and an explicit `now`, so the whole
/// state machine — first call, normal call, too-soon call — is unit-testable
/// without a clock.
pub fn advance(
    previous: &mut Option<NetSample>,
    last_render: &mut String,
    rx: u64,
    tx: u64,
    now: Instant,
    net: &crate::config::Network,
    bar_bg: &str,
) -> String {
    let Some(prev) = *previous else {
        // Nothing to difference against yet; anchor and draw nothing.
        *previous = Some(NetSample { rx, tx, at: now });
        return String::new();
    };

    let elapsed = now.duration_since(prev.at);
    if elapsed < MIN_ELAPSED {
        // Deliberately leave `previous` alone: the next call then measures from
        // the older anchor and has a full-length interval to divide by.
        return last_render.clone();
    }

    let dl = rate(rx.saturating_sub(prev.rx), elapsed);
    let ul = rate(tx.saturating_sub(prev.tx), elapsed);
    *previous = Some(NetSample { rx, tx, at: now });
    *last_render = format_rates(dl, ul, net, bar_bg);
    last_render.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped defaults, which is what every expectation here is written
    /// against.
    fn cfg() -> crate::config::Network {
        crate::config::Network::default()
    }

    /// The bar background the defaults assume.
    const BAR: &str = "colour233";

    // ── iec_fmt ──────────────────────────────────────────────────────────────

    #[test]
    fn iec_fmt_bytes() {
        // iec_fmt(500, 6): format!("{:>6}B/s", 500) = "   500B/s" (3 spaces + 500)
        assert_eq!(iec_fmt(500, 6), "   500B/s");
        assert_eq!(iec_fmt(500, 0), "500B/s");
    }

    #[test]
    fn iec_fmt_kib() {
        assert_eq!(iec_fmt(2048, 0), format!("2{}", RATE_KIB));
        assert_eq!(iec_fmt(1024, 0), format!("1{}", RATE_KIB));
    }

    #[test]
    fn iec_fmt_mib() {
        assert_eq!(iec_fmt(1024 * 1024, 0), format!("1{}", RATE_MIB));
        assert_eq!(iec_fmt(3 * 1024 * 1024, 0), format!("3{}", RATE_MIB));
    }

    #[test]
    fn iec_fmt_gib() {
        assert_eq!(iec_fmt(2 * 1024 * 1024 * 1024, 0), format!("2{}", RATE_GIB));
    }

    #[test]
    fn iec_fmt_units_are_distinct() {
        assert_ne!(RATE_KIB, RATE_MIB);
        assert_ne!(RATE_MIB, RATE_GIB);
        assert_ne!(RATE_KIB, RATE_GIB);
    }

    #[test]
    fn rate_units_never_start_with_a_digit() {
        // iec_fmt_styled splits the number from the unit at the first non-digit
        // character, so a unit that opened with a digit would be mis-split.
        for unit in [RATE_KIB, RATE_MIB, RATE_GIB] {
            let first = unit.chars().next().expect("unit is non-empty");
            assert!(
                !first.is_ascii_digit(),
                "unit must not start with a digit: {unit}"
            );
        }
    }

    #[test]
    fn iec_fmt_padding() {
        // Padding pads on the left
        let s = iec_fmt(1, 8);
        // "       1B/s" — 8 chars wide for the number
        assert_eq!(s, "       1B/s");
    }

    #[test]
    fn iec_fmt_boundary_exactly_kib() {
        // 1024 B/s = exactly 1 KiB/s
        assert_eq!(iec_fmt(1024, 0), format!("1{}", RATE_KIB));
        // 1023 B/s stays in B
        assert_eq!(iec_fmt(1023, 0), "1023B/s");
    }

    // ── format_rates ─────────────────────────────────────────────────────

    #[test]
    fn format_rates_both_below_threshold_empty() {
        assert_eq!(format_rates(100, 100, &cfg(), BAR), "");
    }

    #[test]
    fn format_rates_exactly_at_threshold_empty() {
        // threshold is >=, so 20479 is below and 20480 shows
        assert_eq!(format_rates(THRESHOLD_BPS - 1, 0, &cfg(), BAR), "");
    }

    #[test]
    fn format_rates_dl_above_threshold() {
        let out = format_rates(THRESHOLD_BPS, 0, &cfg(), BAR);
        assert!(out.contains(ARROW_LEFT), "missing arrow: {out}");
        assert!(out.contains("#[fg=#5cae36]"), "expected green arrow: {out}");
        assert!(
            out.contains("#[fg=colour233,bg=#5cae36]"),
            "expected green segment: {out}"
        );
        assert!(!out.contains("#[fg=#0262a8"), "should not have ul: {out}");
    }

    #[test]
    fn format_rates_ul_above_threshold() {
        let out = format_rates(0, THRESHOLD_BPS, &cfg(), BAR);
        assert!(out.contains(ARROW_LEFT), "missing arrow: {out}");
        assert!(out.contains("#[fg=#0262a8]"), "expected blue arrow: {out}");
        assert!(
            out.contains("#[fg=colour233,bg=#0262a8]"),
            "expected blue segment: {out}"
        );
        assert!(!out.contains("#[fg=#5cae36"), "should not have dl: {out}");
    }

    #[test]
    fn format_rates_both_above_threshold() {
        let out = format_rates(THRESHOLD_BPS * 10, THRESHOLD_BPS * 2, &cfg(), BAR);
        assert!(out.contains("#[fg=#5cae36"), "missing dl: {out}");
        assert!(out.contains("#[fg=#0262a8"), "missing ul: {out}");
    }

    #[test]
    fn format_rates_no_leading_space_in_speed() {
        // Number part must immediately follow the color tag — no numeric padding.
        // Use 40 KiB/s (above 20 KiB/s threshold).
        let out = format_rates(40 * 1024, 0, &cfg(), BAR);
        assert!(out.contains("40"), "number present: {out}");
        assert!(!out.contains("   40"), "no left numeric padding: {out}");
    }

    // ── iec_fmt_styled ────────────────────────────────────────────────────────

    #[test]
    fn iec_fmt_styled_kib() {
        assert_eq!(
            iec_fmt_styled(2048, &cfg().unit_colour),
            format!("2#[fg=colour237,none,italics]{}#[none]", RATE_KIB)
        );
    }

    #[test]
    fn iec_fmt_styled_mib() {
        assert_eq!(
            iec_fmt_styled(3 * 1024 * 1024, &cfg().unit_colour),
            format!("3#[fg=colour237,none,italics]{}#[none]", RATE_MIB)
        );
    }

    #[test]
    fn iec_fmt_styled_bytes() {
        assert_eq!(
            iec_fmt_styled(500, &cfg().unit_colour),
            "500#[fg=colour237,none,italics]B/s#[none]"
        );
    }

    #[test]
    fn iec_fmt_styled_split_is_at_first_non_digit() {
        // Verify number and unit are correctly separated for all unit types.
        for (bps, expected_num, expected_unit) in [
            (500_u64, "500", "B/s"),
            (2 * 1024, "2", RATE_KIB),
            (5 * 1024 * 1024, "5", RATE_MIB),
            (2 * 1024 * 1024 * 1024, "2", RATE_GIB),
        ] {
            let s = iec_fmt_styled(bps, &cfg().unit_colour);
            assert!(s.starts_with(expected_num), "num for {bps}: {s}");
            assert!(s.contains(expected_unit), "unit for {bps}: {s}");
            assert!(s.contains("#[fg=colour237,none,italics]"), "style tag: {s}");
            assert!(s.ends_with("#[none]"), "reset tag: {s}");
        }
    }

    #[test]
    fn format_rates_unit_is_styled() {
        let out = format_rates(THRESHOLD_BPS, 0, &cfg(), BAR);
        assert!(
            out.contains("#[fg=colour237,none,italics]"),
            "unit style present: {out}"
        );
        assert!(out.contains("#[none]"), "unit reset present: {out}");
    }

    #[test]
    fn format_rates_shows_human_readable_speed() {
        let out = format_rates(2 * 1024 * 1024, 0, &cfg(), BAR); // 2 MiB/s
        assert!(out.contains(RATE_MIB), "expected MiB glyph in: {out}");
    }

    // ── rate ─────────────────────────────────────────────────────────────────

    #[test]
    fn rate_over_exactly_one_second_is_the_delta() {
        assert_eq!(rate(1000, Duration::from_secs(1)), 1000);
    }

    #[test]
    fn rate_does_not_truncate_the_interval() {
        // The bug this replaces: `as_secs()` turned 1.1s into 1s and reported
        // 1000 B/s where the true rate is ~909 B/s — about 10% high.
        let r = rate(1000, Duration::from_millis(1100));
        assert_eq!(r, 909, "1000 bytes over 1.1s is 909 B/s, not 1000");
        assert_ne!(r, 1000, "must not truncate the interval to whole seconds");
    }

    #[test]
    fn rate_scales_up_for_sub_second_intervals() {
        // Half a second of 500 bytes is a 1000 B/s rate.  The old code divided
        // by max(1) and reported 500 — half the truth.
        assert_eq!(rate(500, Duration::from_millis(500)), 1000);
    }

    #[test]
    fn rate_scales_down_for_multi_second_intervals() {
        assert_eq!(rate(3000, Duration::from_secs(3)), 1000);
        assert_eq!(rate(1000, Duration::from_millis(2500)), 400);
    }

    #[test]
    fn rate_of_zero_elapsed_is_zero_not_a_panic() {
        assert_eq!(rate(1000, Duration::ZERO), 0);
    }

    #[test]
    fn rate_of_zero_delta_is_zero() {
        assert_eq!(rate(0, Duration::from_secs(1)), 0);
    }

    // ── advance: the render state machine ────────────────────────────────────

    #[test]
    fn advance_first_call_draws_nothing_and_anchors() {
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        let out = advance(&mut prev, &mut last, 1000, 2000, t0, &cfg(), BAR);
        assert_eq!(out, "", "nothing to difference against on the first call");
        let anchored = prev.expect("first call must store a sample");
        assert_eq!(anchored.rx, 1000);
        assert_eq!(anchored.tx, 2000);
        assert_eq!(anchored.at, t0);
    }

    #[test]
    fn advance_second_call_reports_the_rate() {
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        advance(&mut prev, &mut last, 0, 0, t0, &cfg(), BAR);
        // 40 KiB down in one second — above the 20 KiB/s threshold.
        let out = advance(
            &mut prev,
            &mut last,
            40 * 1024,
            0,
            t0 + Duration::from_secs(1),
            &cfg(),
            BAR,
        );
        assert!(out.contains(RATE_KIB), "expected a KiB/s rate: {out}");
        assert!(out.contains("40"), "expected 40 KiB/s: {out}");
    }

    #[test]
    fn advance_uses_fractional_seconds_end_to_end() {
        // 44 KiB over 1.1s is 40 KiB/s.  With the old truncating arithmetic it
        // would have reported 44 KiB/s.
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        advance(&mut prev, &mut last, 0, 0, t0, &cfg(), BAR);
        let out = advance(
            &mut prev,
            &mut last,
            44 * 1024,
            0,
            t0 + Duration::from_millis(1100),
            &cfg(),
            BAR,
        );
        assert!(out.contains("40"), "expected 40 KiB/s, got: {out}");
        assert!(
            !out.contains("44"),
            "must not report the untruncated 44: {out}"
        );
    }

    #[test]
    fn advance_below_min_elapsed_replays_the_previous_render() {
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        advance(&mut prev, &mut last, 0, 0, t0, &cfg(), BAR);
        let first = advance(
            &mut prev,
            &mut last,
            40 * 1024,
            0,
            t0 + Duration::from_secs(1),
            &cfg(),
            BAR,
        );
        assert!(!first.is_empty());

        // A second client refreshes 50ms later: far too short an interval to
        // divide a few bytes by.
        let anchor_before = prev.expect("anchored");
        let out = advance(
            &mut prev,
            &mut last,
            40 * 1024 + 10,
            0,
            t0 + Duration::from_millis(1050),
            &cfg(),
            BAR,
        );
        assert_eq!(out, first, "should replay the previous rendering verbatim");
        assert_eq!(
            prev.expect("still anchored"),
            anchor_before,
            "the anchor must not move, so the next call gets a full interval"
        );
    }

    #[test]
    fn advance_just_above_min_elapsed_computes_a_fresh_rate() {
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        advance(&mut prev, &mut last, 0, 0, t0, &cfg(), BAR);
        let at = t0 + MIN_ELAPSED + Duration::from_millis(1);
        let out = advance(&mut prev, &mut last, 100 * 1024, 0, at, &cfg(), BAR);
        assert!(
            !out.is_empty(),
            "just past the guard it must compute: {out}"
        );
        assert_eq!(prev.expect("anchored").at, at, "anchor must advance");
    }

    #[test]
    fn advance_exactly_at_min_elapsed_computes() {
        // The guard is `elapsed < MIN_ELAPSED`, so the boundary itself is fine.
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        advance(&mut prev, &mut last, 0, 0, t0, &cfg(), BAR);
        let at = t0 + MIN_ELAPSED;
        advance(&mut prev, &mut last, 100 * 1024, 0, at, &cfg(), BAR);
        assert_eq!(prev.expect("anchored").at, at);
    }

    #[test]
    fn advance_replays_an_empty_render_when_that_was_the_last_one() {
        // Quiet network: the previous render was "" and the too-soon path must
        // reproduce that rather than inventing a segment.
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        advance(&mut prev, &mut last, 0, 0, t0, &cfg(), BAR);
        let quiet = advance(
            &mut prev,
            &mut last,
            10,
            0,
            t0 + Duration::from_secs(1),
            &cfg(),
            BAR,
        );
        assert_eq!(quiet, "", "10 B/s is below the threshold");
        let soon = advance(
            &mut prev,
            &mut last,
            20,
            0,
            t0 + Duration::from_millis(1050),
            &cfg(),
            BAR,
        );
        assert_eq!(soon, "");
    }

    #[test]
    fn advance_handles_counter_reset_without_underflow() {
        // Interface goes away and comes back: the cumulative counter drops.
        // saturating_sub must keep this at zero rather than wrapping.
        let mut prev = None;
        let mut last = String::new();
        let t0 = Instant::now();
        advance(&mut prev, &mut last, 1_000_000, 1_000_000, t0, &cfg(), BAR);
        let out = advance(
            &mut prev,
            &mut last,
            5,
            5,
            t0 + Duration::from_secs(1),
            &cfg(),
            BAR,
        );
        assert_eq!(out, "", "a counter reset must not report a huge rate");
    }
}
