//! Battery percentage and icon, read through `starship-battery`, the maintained fork of the `battery` crate.
use crate::tmux::icons::{
    BATTERY_EMPTY, BATTERY_FULL, BATTERY_HALF, BATTERY_QUARTER, BATTERY_THREE_QUARTERS, PLUGGED,
};

/// Battery icons: 10 levels, empty to full, as constants in `tmux/icons.rs`
/// so the glyph preset and `[glyphs.icons]` reach them.
const BATTERY_ICONS: [&str; 10] = [
    BATTERY_EMPTY,
    BATTERY_QUARTER,
    BATTERY_QUARTER,
    BATTERY_HALF,
    BATTERY_HALF,
    BATTERY_THREE_QUARTERS,
    BATTERY_THREE_QUARTERS,
    BATTERY_FULL,
    BATTERY_FULL,
    BATTERY_FULL,
];

/// Drawn beside the percentage while the machine is on mains power.
pub const CHARGING_ICON: &str = PLUGGED;

// Color thresholds: red ≤10%, orange ≤30%, yellow ≤60%, green >60%
fn battery_color(pct: u64) -> &'static str {
    match pct {
        0..=10 => "#[fg=#e74646]",
        11..=30 => "#[fg=#d05743]",
        31..=60 => "#[fg=#a8a337]",
        _ => "#[fg=#5cae36]",
    }
}

/// Pure formatting logic extracted so it can be unit-tested without I/O.
///
/// The percentage is always drawn, because the colour and the icon alone say
/// the level to nobody who can't tell red from green or whose font lacks the
/// glyph. The icon is the level, not an animation: a bar that moves every
/// second while charging pulls the eye away from whatever it was on.
fn format_battery_output(current: u64, max: u64, external: bool) -> String {
    if current == 0 {
        return String::new();
    }
    let max = max.max(1);
    let pct = ((current * 100) / max).min(100);
    let color = battery_color(pct);
    let icon = BATTERY_ICONS[((pct * 9) / 100) as usize];
    let plug = if external {
        format!(" {}", CHARGING_ICON)
    } else {
        String::new()
    };
    format!("{color}{icon} {pct}%{plug}")
}

/// Whether the battery read failed because the machine has no power-supply
/// class at all, which is a machine with no battery rather than a failure.
///
/// On Linux the crate lists `/sys/class/power_supply`, and a container or a
/// VM can have no such directory. Treated as an error, it put `timer` on the
/// health mark every render and hid whatever else the mark had to say, which
/// is how it was found: under `act`, the config mark never showed.
fn nowhere_to_look(e: &battery::Error) -> bool {
    std::error::Error::source(e)
        .and_then(|s| s.downcast_ref::<std::io::Error>())
        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
}

/// Read the battery and render percentage, icon and charging state.
pub async fn render() -> anyhow::Result<String> {
    tokio::task::spawn_blocking(|| {
        use battery::units::ratio::ratio;

        let manager = battery::Manager::new()?;
        let mut batteries = match manager.batteries() {
            Ok(batteries) => batteries,
            Err(e) if nowhere_to_look(&e) => return Ok(String::new()),
            Err(e) => return Err(e.into()),
        };
        let b = match batteries.next() {
            Some(Ok(b)) => b,
            _ => return Ok(String::new()),
        };

        let soc: f32 = b.state_of_charge().get::<ratio>(); // 0.0..=1.0
        let current = (soc * 100.0) as u64;
        let external = matches!(b.state(), battery::State::Charging | battery::State::Full);

        Ok(format_battery_output(current, 100, external))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_power_supply_directory_is_no_battery_and_anything_else_is_a_failure() {
        let missing = battery::Error::from(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(nowhere_to_look(&missing));
        let denied =
            battery::Error::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(!nowhere_to_look(&denied));
    }

    #[test]
    fn color_red_at_zero() {
        assert_eq!(battery_color(0), "#[fg=#e74646]");
        assert_eq!(battery_color(10), "#[fg=#e74646]");
    }

    #[test]
    fn color_orange_low() {
        assert_eq!(battery_color(11), "#[fg=#d05743]");
        assert_eq!(battery_color(30), "#[fg=#d05743]");
    }

    #[test]
    fn color_yellow_mid() {
        assert_eq!(battery_color(31), "#[fg=#a8a337]");
        assert_eq!(battery_color(60), "#[fg=#a8a337]");
    }

    #[test]
    fn color_green_high() {
        assert_eq!(battery_color(61), "#[fg=#5cae36]");
        assert_eq!(battery_color(100), "#[fg=#5cae36]");
    }

    #[test]
    fn icons_count_is_ten() {
        assert_eq!(BATTERY_ICONS.len(), 10);
        for icon in &BATTERY_ICONS {
            assert!(!icon.is_empty());
        }
    }

    #[test]
    fn zero_capacity_returns_empty() {
        assert_eq!(format_battery_output(0, 100, false), "");
    }

    #[test]
    fn discharging_shows_percentage() {
        let out = format_battery_output(80, 100, false);
        assert!(out.contains("80%"), "expected 80% in: {out}");
        assert!(out.contains("#[fg=#5cae36]"), "expected green: {out}");
        assert!(!out.contains(CHARGING_ICON), "should not show plug: {out}");
    }

    #[test]
    fn on_mains_the_percentage_stays_and_the_plug_is_added() {
        let out = format_battery_output(80, 100, true);
        assert!(
            out.contains("80%"),
            "the level in words, plugged in too: {out}"
        );
        assert!(out.ends_with(CHARGING_ICON), "should show plug: {out}");
    }

    #[test]
    fn the_icon_is_the_level_and_does_not_move() {
        let a = format_battery_output(50, 100, true);
        let b = format_battery_output(50, 100, true);
        assert_eq!(a, b);
        assert!(a.contains(BATTERY_HALF), "{a}");
    }

    #[test]
    fn icon_index_matches_percentage() {
        let low = format_battery_output(5, 100, false);
        let full = format_battery_output(100, 100, false);
        assert!(low.contains(BATTERY_ICONS[0]));
        assert!(full.contains(BATTERY_ICONS[9]));
    }

    #[test]
    fn percentage_calculated_correctly() {
        // 60/80 = 75%
        let out = format_battery_output(60, 80, false);
        assert!(out.contains("75%"), "expected 75% in: {out}");
    }
}
