//! Short sounds for events: an agent that asked, one that finished, a health
//! reason that appeared.
//!
//! A sighted person glances at the bar and sees the waiting count go up or
//! the health mark come on. Somebody listening hears nothing until they go
//! and read the bar. A tone is the glance: it says something happened without
//! saying what, which `inbox`, `brief` and `read-bar` are for. It can't talk
//! over a screen reader the way speech can, and it doesn't need to know which
//! reader is running, which is why it comes before speech
//! (`docs/a11y/speech-plan-revised.md`).
//!
//! Off by default, held by quiet hours like every other nag, and each event's
//! command can be set or left to the platform's own sound.

use crate::config::{EarconEvent, Earcons};

/// The platform's own sound for an event: one of macOS's system sounds
/// through `afplay`, or a freedesktop theme sound through `paplay`. Distinct
/// per event, so the three can be told apart by ear. Empty on a platform
/// with neither, which plays nothing.
pub fn platform_command(event: EarconEvent, os: &str) -> Vec<String> {
    let (mac, linux) = match event {
        EarconEvent::Asked => ("Glass", "message-new-instant"),
        EarconEvent::Done => ("Pop", "complete"),
        EarconEvent::Health => ("Basso", "dialog-warning"),
        EarconEvent::Chunk => ("Hero", "alarm-clock-elapsed"),
    };
    match os {
        "macos" => vec![
            "afplay".to_string(),
            format!("/System/Library/Sounds/{mac}.aiff"),
        ],
        "linux" => vec![
            "paplay".to_string(),
            format!("/usr/share/sounds/freedesktop/stereo/{linux}.oga"),
        ],
        _ => Vec::new(),
    }
}

/// The command an event plays, or `None` when it plays nothing: sounds off,
/// the event not in `on`, or no command and no platform sound.
pub fn command_for(config: &Earcons, event: EarconEvent, os: &str) -> Option<Vec<String>> {
    if !config.enabled || !config.on.contains(&event) {
        return None;
    }
    command_regardless(config, event, os)
}

/// The command an event would play if it were on: what `earcon` plays, so
/// somebody can hear a sound before turning it on.
pub fn command_regardless(config: &Earcons, event: EarconEvent, os: &str) -> Option<Vec<String>> {
    let set = match event {
        EarconEvent::Asked => &config.asked,
        EarconEvent::Done => &config.done,
        EarconEvent::Health => &config.health,
        EarconEvent::Chunk => &config.chunk,
    };
    let command = if set.is_empty() {
        platform_command(event, os)
    } else {
        set.clone()
    };
    (!command.is_empty()).then_some(command)
}

/// Start a command and leave it: a sound is not worth waiting for, and the
/// loop that noticed the event has the next pass to get on with. A command
/// that isn't there plays nothing, which is what an unset sound on a
/// machine without one should do.
pub fn play(command: &[String]) {
    let Some((program, args)) = command.split_first() else {
        return;
    };
    let _ = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Play an event's sound, if it has one and quiet hours aren't on.
pub fn sound(config: &Earcons, event: EarconEvent, quiet: bool) {
    if quiet {
        return;
    }
    if let Some(command) = command_for(config, event, std::env::consts::OS) {
        play(&command);
    }
}

/// The reasons in `now` that were not in `before`. A reason that stays up
/// is not news on the next pass; one that went and came back is.
pub fn new_reasons<'a>(before: &[String], now: &'a [String]) -> Vec<&'a String> {
    now.iter().filter(|r| !before.contains(r)).collect()
}

/// How often the health check runs for the sound, in seconds. The bar's own
/// check is behind a five-second cache and runs only while a bar draws it;
/// somebody who hears the screen may have no bar at all.
pub const HEALTH_EVERY_SECS: u64 = 10;

/// The daemon's task: check health on a timer and sound when a reason
/// appears.
///
/// Nothing is checked or remembered while quiet hours are on, so a reason
/// that arrived during them still sounds when they end.
pub async fn health_loop(
    config: Earcons,
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
) {
    let mut last: Vec<String> = Vec::new();
    let mut first = true;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(HEALTH_EVERY_SECS)).await;
        if state.lock().await.is_quiet() {
            continue;
        }
        let reasons = crate::server::health_check(&state).await.reasons;
        // The first pass only learns what is already wrong: a daemon that
        // starts into a known reason shouldn't sound for it.
        if !first && !new_reasons(&last, &reasons).is_empty() {
            sound(&config, EarconEvent::Health, false);
        }
        first = false;
        last = reasons;
    }
}

/// `earcon <event>`: play an event's sound now, whether sounds are on or
/// not, and say which command it ran.
pub fn run(event: EarconEvent) -> anyhow::Result<()> {
    let config = crate::cli::config_or_default();
    let Some(command) = command_regardless(&config.earcons, event, std::env::consts::OS) else {
        println!("No sound for that here: set one under [earcons] in the config.");
        return Ok(());
    };
    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .status();
    match status {
        Ok(s) if s.success() => println!("Played: {}", command.join(" ")),
        Ok(s) => println!("{} exited with {s}", command.join(" ")),
        Err(e) => println!("Could not run {}: {e}", command[0]),
    }
    if !config.earcons.enabled {
        println!("Sounds are off; [earcons] enabled = true turns them on.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_event_has_its_own_sound_on_each_platform() {
        let mac: Vec<_> = [EarconEvent::Asked, EarconEvent::Done, EarconEvent::Health]
            .map(|e| platform_command(e, "macos")[1].clone())
            .into();
        assert_eq!(
            mac,
            [
                "/System/Library/Sounds/Glass.aiff",
                "/System/Library/Sounds/Pop.aiff",
                "/System/Library/Sounds/Basso.aiff"
            ]
        );
        assert_eq!(platform_command(EarconEvent::Health, "linux")[0], "paplay");
        assert!(platform_command(EarconEvent::Asked, "windows").is_empty());
    }

    #[test]
    fn off_or_not_listed_plays_nothing_and_a_set_command_wins() {
        let mut c = Earcons::default();
        assert_eq!(command_for(&c, EarconEvent::Asked, "macos"), None);
        c.enabled = true;
        assert!(command_for(&c, EarconEvent::Asked, "macos").is_some());
        // done is not in the default `on`.
        assert_eq!(command_for(&c, EarconEvent::Done, "macos"), None);
        c.asked = vec!["printf".into(), "\\a".into()];
        assert_eq!(
            command_for(&c, EarconEvent::Asked, "macos"),
            Some(vec!["printf".to_string(), "\\a".to_string()])
        );
        // `earcon` plays one that is off, so it can be heard first.
        assert!(command_regardless(&c, EarconEvent::Done, "macos").is_some());
    }

    #[test]
    fn only_a_reason_that_was_not_there_is_new() {
        let before = vec!["timer failed".to_string()];
        let now = vec!["offline 2m".to_string(), "timer failed".to_string()];
        assert_eq!(new_reasons(&before, &now), [&now[0]]);
        assert!(new_reasons(&now, &before).is_empty());
    }
}
