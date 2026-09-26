//! Telling you a long command finished in a pane you were not looking at.
//!
//! The whole value is in the last clause. A command that finishes in front of
//! you needs no announcement, so a notification that fires for those is noise
//! that trains people to ignore the ones that matter.
//!
//! This is a poll rather than a hook because tmux has no event for "the
//! foreground process in this pane changed". One `list-panes -a` per interval
//! is the entire cost, and it happens inside a daemon that is already running.
//!
//! Credit: `rickstaa/tmux-notify` is this idea, it is alive, and somebody not
//! running tmux-companion should install theirs.
//!
//! The default notifier is tmux's own `display-message`, which needs nothing
//! installed and works the same on every platform. A desktop notification is a
//! `command` in the config away, and is deliberately not the default: shelling
//! out to `osascript` or `notify-send` on a machine that has neither is a
//! failure somebody has to debug, and the point of the default is that it
//! works.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// One pane as tmux reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    /// `#{pane_id}`.
    pub id: String,
    /// `#{pane_current_command}`.
    pub command: String,
    /// Whether somebody could see this pane at the moment of the scan.
    pub visible: bool,
}

/// What the tracker remembers about a pane between passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// What was running last time.
    pub command: String,
    /// When it started, as far as this can tell.
    pub since: Instant,
    /// Whether the pane has been out of sight at any point since it started.
    ///
    /// Sampled over the whole run rather than at the end, because the usual
    /// shape is starting something, switching away, and coming back when it is
    /// already done: at the moment it finishes the pane is visible again and a
    /// check taken then would say you had been watching all along.
    pub was_hidden: bool,
}

/// A command that finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// The pane it finished in.
    pub pane: String,
    /// What finished.
    pub command: String,
    /// How long it ran, as far as a poll can tell.
    pub ran_for: Duration,
}

/// The `-F` string the scan asks for.
pub fn pane_format() -> &'static str {
    "#{pane_id}\t#{pane_current_command}\t#{pane_active}\t#{window_active}\t#{session_attached}"
}

/// Parse what [`pane_format`] produces.
///
/// A pane is visible when it is the active pane of the active window of a
/// session somebody is attached to. Any of those being false means nobody saw
/// what it printed.
pub fn parse_panes(text: &str) -> Vec<PaneRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(5, '\t').collect();
            if f.len() != 5 {
                return None;
            }
            let on = |s: &str| s.trim() == "1";
            let attached: u32 = f[4].trim().parse().unwrap_or(0);
            Some(PaneRow {
                id: f[0].to_string(),
                command: f[1].to_string(),
                visible: on(f[2]) && on(f[3]) && attached > 0,
            })
        })
        .collect()
}

/// Whether a command is one worth timing.
///
/// The ignore list is the feature, not a detail. An editor, a pager or an agent
/// runs for hours and finishing one is not news, and without the list every
/// `:q` would fire a notification about a two-hour nvim session.
pub fn interesting(command: &str, shell: &str, ignore: &[String]) -> bool {
    let name = command.trim();
    if name.is_empty() {
        return false;
    }
    let shell_name = shell.rsplit('/').next().unwrap_or(shell);
    if name == shell_name || name == shell {
        return false;
    }
    !ignore.iter().any(|i| i == name)
}

/// Everything one pass needs that is not the pane list.
#[derive(Debug, Clone)]
pub struct Rules<'a> {
    /// The login shell, which is what a pane at a prompt reports.
    pub shell: &'a str,
    /// Commands never worth announcing.
    pub ignore: &'a [String],
    /// How long something has to run before finishing is news.
    pub threshold: Duration,
    /// Whether to announce something that finished in front of you.
    pub only_when_unwatched: bool,
}

/// Advance the tracker by one scan, reporting what finished.
///
/// A pane that vanished between passes is dropped without a notification:
/// whatever was running in it went with the pane, and the person who closed it
/// knows.
pub fn step(
    watched: &mut HashMap<String, Watched>,
    rows: &[PaneRow],
    rules: &Rules,
    now: Instant,
) -> Vec<Finished> {
    let mut out = Vec::new();
    let live: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    watched.retain(|id, _| live.contains(id.as_str()));

    for row in rows {
        match watched.get_mut(&row.id) {
            Some(prev) if prev.command == row.command => {
                prev.was_hidden |= !row.visible;
            }
            Some(prev) => {
                let ran = now.saturating_duration_since(prev.since);
                let worth_timing = interesting(&prev.command, rules.shell, rules.ignore);
                let unwatched = prev.was_hidden || !row.visible;
                if worth_timing
                    && ran >= rules.threshold
                    && (!rules.only_when_unwatched || unwatched)
                {
                    out.push(Finished {
                        pane: row.id.clone(),
                        command: prev.command.clone(),
                        ran_for: ran,
                    });
                }
                prev.command = row.command.clone();
                prev.since = now;
                prev.was_hidden = !row.visible;
            }
            // First sight of a pane. Nothing is announced, because this cannot
            // tell how long whatever is in it has already been running, and a
            // daemon restart would otherwise announce everything at once.
            None => {
                watched.insert(
                    row.id.clone(),
                    Watched {
                        command: row.command.clone(),
                        since: now,
                        was_hidden: !row.visible,
                    },
                );
            }
        }
    }
    out
}

/// Seconds as `2m 05s`, or `45s`.
pub fn human(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

/// What to say about a finished command.
pub fn message(f: &Finished) -> String {
    format!("{} finished after {}", f.command, human(f.ran_for))
}

/// The command that announces one finish.
///
/// An empty `command` means tmux's own `display-message`, which needs nothing
/// installed. A configured one gets the message as its last argument, and
/// `{command}`, `{duration}` and `{pane}` are substituted anywhere they appear
/// so a desktop notifier can be given a title and a body.
pub fn notify_command(f: &Finished, configured: &[String]) -> Vec<String> {
    if configured.is_empty() {
        return vec![
            "tmux".to_string(),
            "display-message".to_string(),
            "-d".to_string(),
            "4000".to_string(),
            format!("tmux-companion: {}", message(f)),
        ];
    }
    configured
        .iter()
        .map(|a| {
            a.replace("{command}", &f.command)
                .replace("{duration}", &human(f.ran_for))
                .replace("{pane}", &f.pane)
                .replace("{message}", &message(f))
        })
        .collect()
}

/// The task: scan, compare, announce.
pub async fn notify_loop(
    settings: crate::config::Notify,
    shell: String,
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
) {
    let interval = Duration::from_secs(settings.interval_secs.max(1));
    let rules_threshold = Duration::from_secs(settings.threshold_secs);
    let mut watched: HashMap<String, Watched> = HashMap::new();
    loop {
        tokio::time::sleep(interval).await;
        let listing = tokio::process::Command::new("tmux")
            .args(["list-panes", "-a", "-F", pane_format()])
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
        let rules = Rules {
            shell: &shell,
            ignore: &settings.ignore,
            threshold: rules_threshold,
            only_when_unwatched: settings.only_when_unwatched,
        };
        // The scan still runs while quiet, so nothing finishes twice when it
        // ends; only the announcement is held back.
        let quiet = state.lock().await.is_quiet();
        for f in step(&mut watched, &parse_panes(&listing), &rules, Instant::now()) {
            if quiet {
                continue;
            }
            let cmd = notify_command(&f, &settings.command);
            let Some((program, args)) = cmd.split_first() else {
                continue;
            };
            let _ = tokio::process::Command::new(program)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules<'a>(ignore: &'a [String], only_unwatched: bool) -> Rules<'a> {
        Rules {
            shell: "/bin/zsh",
            ignore,
            threshold: Duration::from_secs(30),
            only_when_unwatched: only_unwatched,
        }
    }

    fn pane(command: &str, visible: bool) -> Vec<PaneRow> {
        vec![PaneRow {
            id: "%1".to_string(),
            command: command.to_string(),
            visible,
        }]
    }

    #[test]
    fn the_first_scan_announces_nothing() {
        // A daemon restart would otherwise announce everything running
        // everywhere, and it cannot know how long any of it has been going.
        let mut w = HashMap::new();
        let t0 = Instant::now();
        assert!(step(&mut w, &pane("cargo", false), &rules(&[], true), t0).is_empty());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn a_long_command_finishing_out_of_sight_is_announced() {
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], true);
        step(&mut w, &pane("cargo", false), &r, t0);
        let out = step(
            &mut w,
            &pane("zsh", false),
            &r,
            t0 + Duration::from_secs(60),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].command, "cargo");
        assert_eq!(out[0].ran_for, Duration::from_secs(60));
    }

    #[test]
    fn a_short_command_is_not_announced() {
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], true);
        step(&mut w, &pane("cargo", false), &r, t0);
        assert!(step(&mut w, &pane("zsh", false), &r, t0 + Duration::from_secs(5)).is_empty());
    }

    #[test]
    fn a_command_you_watched_the_whole_way_through_is_not_announced() {
        // It finished in front of you. Announcing it is the noise that trains
        // people to ignore the ones that matter.
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], true);
        step(&mut w, &pane("cargo", true), &r, t0);
        step(
            &mut w,
            &pane("cargo", true),
            &r,
            t0 + Duration::from_secs(30),
        );
        assert!(step(&mut w, &pane("zsh", true), &r, t0 + Duration::from_secs(60)).is_empty());
    }

    #[test]
    fn looking_away_and_coming_back_still_counts_as_unwatched() {
        // The usual shape: start something, switch away, come back when it is
        // already done. At the moment it finishes the pane is visible again,
        // so a check taken only at the end would say you had been watching.
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], true);
        step(&mut w, &pane("cargo", true), &r, t0);
        step(
            &mut w,
            &pane("cargo", false),
            &r,
            t0 + Duration::from_secs(20),
        );
        let out = step(&mut w, &pane("zsh", true), &r, t0 + Duration::from_secs(60));
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn only_when_unwatched_off_announces_it_either_way() {
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], false);
        step(&mut w, &pane("cargo", true), &r, t0);
        assert_eq!(
            step(&mut w, &pane("zsh", true), &r, t0 + Duration::from_secs(60)).len(),
            1
        );
    }

    #[test]
    fn an_ignored_command_is_never_announced() {
        // Without the list every :q fires a notification about a two-hour nvim.
        let ignore = vec!["nvim".to_string()];
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&ignore, true);
        step(&mut w, &pane("nvim", false), &r, t0);
        assert!(
            step(
                &mut w,
                &pane("zsh", false),
                &r,
                t0 + Duration::from_secs(7200)
            )
            .is_empty()
        );
    }

    #[test]
    fn a_shell_becoming_a_command_is_not_a_finish() {
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], true);
        step(&mut w, &pane("zsh", false), &r, t0);
        assert!(
            step(
                &mut w,
                &pane("cargo", false),
                &r,
                t0 + Duration::from_secs(60)
            )
            .is_empty()
        );
    }

    #[test]
    fn one_command_replacing_another_announces_the_first() {
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], true);
        step(&mut w, &pane("cargo", false), &r, t0);
        let out = step(
            &mut w,
            &pane("make", false),
            &r,
            t0 + Duration::from_secs(60),
        );
        assert_eq!(out[0].command, "cargo");
        // And the clock restarts for the new one rather than carrying on.
        let out2 = step(
            &mut w,
            &pane("zsh", false),
            &r,
            t0 + Duration::from_secs(70),
        );
        assert!(out2.is_empty(), "{out2:?}");
    }

    #[test]
    fn a_pane_that_closed_announces_nothing_and_is_forgotten() {
        // Whatever was running went with the pane, and the person who closed
        // it knows.
        let mut w = HashMap::new();
        let t0 = Instant::now();
        let r = rules(&[], true);
        step(&mut w, &pane("cargo", false), &r, t0);
        assert!(step(&mut w, &[], &r, t0 + Duration::from_secs(60)).is_empty());
        assert!(w.is_empty(), "the pane should not be remembered");
    }

    #[test]
    fn a_pane_at_a_prompt_is_never_the_thing_that_finished() {
        assert!(!interesting("zsh", "/bin/zsh", &[]));
        assert!(!interesting("/bin/zsh", "/bin/zsh", &[]));
        assert!(!interesting("", "/bin/zsh", &[]));
        assert!(interesting("cargo", "/bin/zsh", &[]));
    }

    #[test]
    fn visibility_needs_the_pane_the_window_and_a_client() {
        let rows = parse_panes(
            "%1\tcargo\t1\t1\t1\n%2\tcargo\t0\t1\t1\n%3\tcargo\t1\t0\t1\n%4\tcargo\t1\t1\t0\n",
        );
        assert!(rows[0].visible, "active pane, active window, attached");
        assert!(!rows[1].visible, "not the active pane");
        assert!(!rows[2].visible, "not the active window");
        assert!(!rows[3].visible, "nobody attached");
    }

    #[test]
    fn a_short_line_is_skipped_rather_than_panicking() {
        assert!(parse_panes("%1\tcargo\n").is_empty());
    }

    #[test]
    fn the_default_notifier_needs_nothing_installed() {
        let f = Finished {
            pane: "%1".into(),
            command: "cargo".into(),
            ran_for: Duration::from_secs(90),
        };
        let cmd = notify_command(&f, &[]);
        assert_eq!(cmd[0], "tmux");
        assert!(cmd.last().unwrap().contains("cargo finished after 1m 30s"));
    }

    #[test]
    fn a_configured_notifier_gets_the_pieces_substituted() {
        let f = Finished {
            pane: "%3".into(),
            command: "cargo".into(),
            ran_for: Duration::from_secs(45),
        };
        let configured = vec![
            "notify-send".to_string(),
            "{command}".to_string(),
            "ran for {duration} in {pane}".to_string(),
        ];
        assert_eq!(
            notify_command(&f, &configured),
            vec!["notify-send", "cargo", "ran for 45s in %3"]
        );
    }

    #[test]
    fn durations_read_the_way_people_say_them() {
        assert_eq!(human(Duration::from_secs(9)), "9s");
        assert_eq!(human(Duration::from_secs(59)), "59s");
        assert_eq!(human(Duration::from_secs(60)), "1m 00s");
        assert_eq!(human(Duration::from_secs(605)), "10m 05s");
    }
}
