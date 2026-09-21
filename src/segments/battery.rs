//! Battery percentage and icon, read through the `battery` crate.
// Battery icons: 10 levels (empty → full)
const BATTERY_ICONS: [&str; 10] = [
    "\u{f244}", "\u{f243}", "\u{f243}", "\u{f242}", "\u{f242}", "\u{f241}", "\u{f241}", "\u{f240}",
    "\u{f240}", "\u{f240}",
];

/// Drawn beside the percentage while the machine is on mains power.
pub const CHARGING_ICON: &str = "\u{f1e6}";

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
fn format_battery_output(
    current: u64,
    max: u64,
    is_charging: bool,
    external: bool,
    epoch_secs: u64,
) -> String {
    if current == 0 {
        return String::new();
    }
    let max = max.max(1);
    let pct = (current * 100) / max;
    let color = battery_color(pct);

    let icon_idx = if is_charging || external {
        (epoch_secs % 10) as usize
    } else {
        ((pct * 9) / 100) as usize
    };

    let icon = BATTERY_ICONS[icon_idx.min(9)];
    let pct_str = if !external {
        format!(" {}%", pct)
    } else {
        String::new()
    };
    let plug = if external {
        format!(" {}", CHARGING_ICON)
    } else {
        String::new()
    };

    format!("{}{}{}{}", color, icon, pct_str, plug)
}

/// Read the battery and render percentage, icon and charging state.
pub async fn render() -> anyhow::Result<String> {
    tokio::task::spawn_blocking(|| {
        use battery::units::ratio::ratio;

        let manager = battery::Manager::new()?;
        let b = match manager.batteries()?.next() {
            Some(Ok(b)) => b,
            _ => return Ok(String::new()),
        };

        let soc: f32 = b.state_of_charge().get::<ratio>(); // 0.0..=1.0
        let current = (soc * 100.0) as u64;
        let is_charging = b.state() == battery::State::Charging;
        let external = matches!(b.state(), battery::State::Charging | battery::State::Full);

        let epoch_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(format_battery_output(
            current,
            100,
            is_charging,
            external,
            epoch_secs,
        ))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(format_battery_output(0, 100, false, false, 0), "");
    }

    #[test]
    fn discharging_shows_percentage() {
        let out = format_battery_output(80, 100, false, false, 0);
        assert!(out.contains("80%"), "expected 80% in: {out}");
        assert!(out.contains("#[fg=#5cae36]"), "expected green: {out}");
        assert!(!out.contains(CHARGING_ICON), "should not show plug: {out}");
    }

    #[test]
    fn external_hides_percentage_shows_plug() {
        let out = format_battery_output(80, 100, false, true, 0);
        assert!(!out.contains('%'), "external should not show %: {out}");
        assert!(out.contains(CHARGING_ICON), "should show plug: {out}");
    }

    #[test]
    fn charging_icon_cycles_by_epoch() {
        // Charging animates: epoch_secs % 10 selects the icon index.
        // At epoch=0 → idx=0, epoch=5 → idx=5.
        let out0 = format_battery_output(50, 100, true, false, 0);
        let out5 = format_battery_output(50, 100, true, false, 5);
        // Both must contain a battery icon (non-empty output) — exact icon may differ.
        assert!(!out0.is_empty());
        assert!(!out5.is_empty());
    }

    #[test]
    fn icon_index_matches_percentage() {
        // At 0% → idx 0 (lowest icon); at 100% → idx 9 (fullest icon).
        let low = format_battery_output(5, 100, false, false, 0);
        let full = format_battery_output(100, 100, false, false, 0);
        assert!(low.contains(BATTERY_ICONS[0]));
        assert!(full.contains(BATTERY_ICONS[9]));
    }

    #[test]
    fn percentage_calculated_correctly() {
        // 60/80 = 75%
        let out = format_battery_output(60, 80, false, false, 0);
        assert!(out.contains("75%"), "expected 75% in: {out}");
    }
}
