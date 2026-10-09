//! the status bar pushed: the daemon renders each attached session's right
//! side and sets it as a session option tmux expands, instead of tmux running
//! a process every second to ask.
//!
//! With `[bar] mode = "push"`, or `auto` (the default) when tmux's global
//! `status-right` reads `@tc-right` and tmux is 3.4 or newer, `status-right`
//! is `#{@tc-right}` (or the line below, which falls back to the call), and
//! once a second this loop:
//!
//! 1. takes the shared pane list (`panes::shared`), whose visible panes are
//!    the active pane of each session somebody is attached to, and so the
//!    path each session's git segment is about;
//! 2. renders the right side for each of those sessions, as `status-right`
//!    would for that path;
//! 3. sends the ones that changed since the last tick in one `tmux`
//!    invocation, `set-option -t =<session>: @tc-right <value> ; ...`, and
//!    nothing at all when none did.
//!
//! The cost is one `tmux` process a second for the whole server at most, and
//! the pane list the loops read anyway, against one shell and one client
//! process per attached client per second in call mode.
//!
//! While it pushes the daemon also writes a heartbeat, the global option
//! `@tc-alive`, every [`BEAT`], and the recommended line ([`line`]) shows the
//! pushed value only while that is under [`STALE_SECS`] old. A daemon that
//! stops cleanly (a signal, `restart`, an upgrade) unsets both at once; one
//! that crashes leaves them, the bar shows old figures for at most
//! [`STALE_SECS`], and then the line's call starts a new daemon, which finds
//! the heartbeat left behind and records the unclean stop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::server::state::ServerState;

/// The oldest tmux push is used on: the floor the suite is tested against.
pub const MIN_TMUX: (u32, u32) = (3, 4);

/// How often `auto` looks at `status-right` again.
pub const RECHECK: Duration = Duration::from_secs(60);

/// What the daemon does with the bar, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Push, and whether `status-right` reads what is pushed.
    Push { read: bool },
    /// Leave the bar to the `#()` call, for this reason.
    Call(Why),
}

/// Why the bar is left to the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// `mode = "call"`.
    Configured,
    /// `auto`, and the global `status-right` does not mention `@tc-right`.
    NotRead,
    /// `auto`, and tmux is older than [`MIN_TMUX`], or did not say.
    OldTmux(String),
}

/// Push or call, from the mode, tmux's global `status-right` and `tmux -V`.
pub fn decide(mode: crate::config::BarMode, status_right: &str, tmux_v: &str) -> Decision {
    use crate::config::BarMode;
    let read = status_right.contains(OPTION);
    match mode {
        BarMode::Call => Decision::Call(Why::Configured),
        BarMode::Push => Decision::Push { read },
        BarMode::Auto => match crate::keyroute::route::tmux_version(tmux_v) {
            Some(v) if v >= MIN_TMUX && read => Decision::Push { read },
            Some(v) if v >= MIN_TMUX => Decision::Call(Why::NotRead),
            _ => Decision::Call(Why::OldTmux(tmux_v.trim().to_string())),
        },
    }
}

/// What `doctor` says about the bar.
pub fn describe(mode: crate::config::BarMode, d: &Decision) -> String {
    let mode = format!("{mode:?}").to_lowercase();
    match d {
        Decision::Push { read: true } => format!("push ({mode})"),
        Decision::Push { read: false } => format!(
            "push ({mode}), but status-right does not read {OPTION}, so nothing shows it; \
             use the status-right line in docs/tmux.conf.example"
        ),
        Decision::Call(Why::Configured) => "call (call)".to_string(),
        Decision::Call(Why::NotRead) => format!(
            "call (auto): status-right does not read {OPTION}; the status-right line in \
             docs/tmux.conf.example pushes, which costs less"
        ),
        Decision::Call(Why::OldTmux(v)) => format!(
            "call (auto): {} is older than tmux {}.{}, which push needs",
            if v.is_empty() { "this tmux" } else { v },
            MIN_TMUX.0,
            MIN_TMUX.1
        ),
    }
}

/// The reason for the health mark, when there is one: only a tmux too old
/// for push. A status-right that does not read the option is a choice, and
/// a mark on a bar that works would be noise; `doctor` says that one.
pub fn health_reason(d: &Decision) -> Option<String> {
    match d {
        Decision::Call(Why::OldTmux(_)) => Some(describe(crate::config::BarMode::Auto, d)),
        _ => None,
    }
}

/// The global option the daemon keeps fresh while it pushes: the unix time
/// of its last heartbeat. The status line reads the pushed value only while
/// this is younger than [`STALE_SECS`], so a daemon that died without
/// cleaning up costs a frozen bar for that long, and then the line's call
/// starts a new one.
pub const ALIVE: &str = "@tc-alive";

/// How often the heartbeat is written when nothing else is.
pub const BEAT: Duration = Duration::from_secs(10);

/// How old a heartbeat the status line still trusts.
pub const STALE_SECS: u64 = 30;

/// Two unclean stops this close together are a pattern, not an accident.
pub const REPEAT_SECS: u64 = 60;

/// The recommended `status-right`, around a call: the pushed value while it
/// is set for this session and the heartbeat is fresh, the call otherwise.
pub fn line(call: &str) -> String {
    format!(
        "#{{?#{{&&:#{{{OPTION}}},#{{e|<:%s,#{{e|+:#{{{ALIVE}}},{STALE_SECS}}}}}}},#{{{OPTION}}},{call}}}"
    )
}

/// Whether the last two lines of the crash file, `unix time<TAB>...` each,
/// are within [`REPEAT_SECS`] of each other.
pub fn repeated(crashes: &str) -> bool {
    let times: Vec<u64> = crashes
        .lines()
        .filter_map(|l| l.split('\t').next()?.trim().parse().ok())
        .collect();
    match times.as_slice() {
        [.., a, b] => b.saturating_sub(*a) <= REPEAT_SECS,
        _ => false,
    }
}

/// A daemon that finds a heartbeat on start knows the last one died without
/// unsetting it: a crash, a `kill -9`, a machine that slept through it. Said
/// in the daemon log and the crash file, and on the health mark, which says
/// more and stays (an hour, or until `health ack`) when it has happened twice
/// within [`REPEAT_SECS`].
async fn note_unclean_stop(state: &Arc<Mutex<ServerState>>, last_beat: &str) {
    let now = crate::panes::now_secs();
    let beat: u64 = last_beat.trim().parse().unwrap_or(0);
    let gap = crate::panes::age(now.saturating_sub(beat));
    eprintln!(
        "tmux-companion: the last daemon stopped without unsetting {ALIVE} \
         (a crash or a kill); its last heartbeat was {gap} before this one started"
    );
    let mut crashes = String::new();
    if let Some(dir) = crate::server::state_dir() {
        let file = dir.join("crashes.tsv");
        crashes = std::fs::read_to_string(&file).unwrap_or_default();
        let entry = format!("{now}\tlast heartbeat {beat}\n");
        crashes.push_str(&entry);
        let _ = std::fs::create_dir_all(&dir);
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }
    let mut st = state.lock().await;
    st.note_unclean(
        "the daemon stopped without cleaning up and was started again; see the daemon log"
            .to_string(),
    );
    if repeated(&crashes) {
        st.note_unclean(format!(
            "the daemon stopped uncleanly twice within {REPEAT_SECS}s; \
             crashes.tsv in the state directory has every one"
        ));
    }
}

/// The option the bar reads.
pub const OPTION: &str = "@tc-right";

/// How often the bar is rendered, which is what `status-interval 1` gave.
pub const TICK: Duration = Duration::from_secs(1);

/// Sessions this daemon has set the option in, for clearing on the way out.
static PUSHED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The session each visible pane belongs to, with that pane's directory: one
/// per session somebody is attached to.
pub fn sessions(panes: &[crate::panes::Pane]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for p in panes.iter().filter(|p| p.visible) {
        if !out.iter().any(|(s, _)| s == &p.session) {
            out.push((p.session.clone(), p.path.clone()));
        }
    }
    out
}

/// The `tmux` arguments that set every changed session's value, as one
/// command sequence, or nothing when nothing changed.
pub fn commands(changed: &[(String, String)]) -> Vec<String> {
    let mut args = Vec::new();
    for (i, (session, value)) in changed.iter().enumerate() {
        if i > 0 {
            args.push(";".to_string());
        }
        args.extend([
            "set-option".to_string(),
            "-t".to_string(),
            // A trailing colon: set-option takes a pane target, and `=name`
            // alone is refused there as "no such session".
            format!("={session}:"),
            OPTION.to_string(),
            value.clone(),
        ]);
    }
    args
}

/// The daemon's task, for `push` and `auto`.
pub async fn push_loop(state: Arc<Mutex<ServerState>>, mode: crate::config::BarMode) {
    let tmux_v = crate::tmux::command()
        .arg("-V")
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut last: HashMap<String, String> = HashMap::new();
    let mut checked: Option<std::time::Instant> = None;
    let mut active = false;
    let mut beat: Option<std::time::Instant> = None;
    let mut looked_for_crash = false;
    loop {
        // `auto` looks at status-right once a minute, which is how long an
        // edited tmux.conf waits to be pushed to; `push` looks once, so
        // `doctor`'s warning has a decision to come from.
        if checked.is_none_or(|t| mode == crate::config::BarMode::Auto && t.elapsed() >= RECHECK) {
            // A session first: with no server, `show-options -g` starts one
            // long enough to print the defaults, which never read the option.
            let sessions = crate::tmux::command()
                .args(["list-sessions", "-F", "x"])
                .output()
                .await
                .ok()
                .filter(|o| o.status.success() && !o.stdout.is_empty());
            let answer = match sessions {
                Some(_) => crate::tmux::command()
                    .args(["show-options", "-gv", "status-right"])
                    .output()
                    .await
                    .ok()
                    .filter(|o| o.status.success()),
                None => None,
            };
            // No server yet (a daemon started before tmux, at login or by a
            // script) is not an answer: decided on it, `auto` would sit on
            // the call for a minute. Ask again next tick instead.
            let Some(answer) = answer else {
                tokio::time::sleep(TICK).await;
                continue;
            };
            let status_right = String::from_utf8_lossy(&answer.stdout).into_owned();
            // Once, on the first answer: a heartbeat already there is one the
            // last daemon left behind.
            if !looked_for_crash {
                looked_for_crash = true;
                let left = crate::tmux::command()
                    .args(["show-options", "-gqv", ALIVE])
                    .output()
                    .await
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                if !left.is_empty() {
                    note_unclean_stop(&state, &left).await;
                }
            }
            let decision = decide(mode, &status_right, &tmux_v);
            let now = matches!(decision, Decision::Push { .. });
            if active && !now {
                clear();
                last.clear();
            }
            active = now;
            state.lock().await.bar_note = health_reason(&decision);
            checked = Some(std::time::Instant::now());
        }
        tokio::time::sleep(TICK).await;
        if !active {
            continue;
        }
        let panes = crate::panes::shared(&state, TICK).await;
        let mut changed = Vec::new();
        for (session, path) in sessions(&panes) {
            let args: crate::proto::StatusRightArgs =
                match serde_json::from_value(serde_json::json!({ "path": path })) {
                    Ok(a) => a,
                    Err(_) => continue,
                };
            let Ok(value) = crate::server::render_right(&args, &state).await else {
                continue;
            };
            if last.get(&session) != Some(&value) {
                last.insert(session.clone(), value.clone());
                changed.push((session, value));
            }
        }
        let beat_due = beat.is_none_or(|t| t.elapsed() >= BEAT);
        if changed.is_empty() && !beat_due {
            continue;
        }
        if let Ok(mut pushed) = PUSHED.lock() {
            for (s, _) in &changed {
                if !pushed.contains(s) {
                    pushed.push(s.clone());
                }
            }
        }
        let mut args = commands(&changed);
        if beat_due {
            if !args.is_empty() {
                args.push(";".to_string());
            }
            args.extend([
                "set-option".to_string(),
                "-g".to_string(),
                ALIVE.to_string(),
                crate::panes::now_secs().to_string(),
            ]);
            beat = Some(std::time::Instant::now());
        }
        let _ = crate::tmux::command()
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
    }
}

/// Unset the option everywhere this daemon set it, so the bar falls back to
/// the call rather than showing a figure nobody is updating. Synchronous: it
/// runs on the way out of a signal handler.
pub fn clear() {
    let sessions = PUSHED.lock().map(|p| p.clone()).unwrap_or_default();
    // The heartbeat first: gone, the line falls back to the call at once, and
    // the next daemon does not take this stop for a crash.
    let mut args: Vec<String> = ["set-option", "-gu", ALIVE]
        .iter()
        .map(|a| a.to_string())
        .collect();
    for s in &sessions {
        args.push(";".to_string());
        args.extend([
            "set-option".to_string(),
            "-u".to_string(),
            "-t".to_string(),
            format!("={s}:"),
            OPTION.to_string(),
        ]);
    }
    let _ = crate::tmux::command_sync()
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BarMode;

    const LINE: &str = "#{?@tc-right,#{@tc-right},#(tmux-companion status-right)}";
    const CALL: &str = "#(tmux-companion status-right)";

    #[test]
    fn the_recommended_line_reads_the_value_only_while_the_heartbeat_is_fresh() {
        assert_eq!(
            line("#(x)"),
            "#{?#{&&:#{@tc-right},#{e|<:%s,#{e|+:#{@tc-alive},30}}},#{@tc-right},#(x)}"
        );
    }

    #[test]
    fn two_unclean_stops_within_a_minute_are_a_repeat() {
        assert!(!repeated(""));
        assert!(!repeated("1000\tlast heartbeat 990\n"));
        assert!(repeated("1000\tx\n1050\tx\n"));
        assert!(!repeated("1000\tx\n1100\tx\n"));
        assert!(repeated("10\tx\n1000\tx\n1030\tx\n"), "the last two decide");
    }

    #[test]
    fn auto_pushes_only_what_status_right_reads_on_a_tmux_new_enough() {
        assert_eq!(
            decide(BarMode::Auto, LINE, "tmux 3.4"),
            Decision::Push { read: true }
        );
        assert_eq!(
            decide(BarMode::Auto, LINE, "tmux next-3.7"),
            Decision::Push { read: true }
        );
        assert_eq!(
            decide(BarMode::Auto, CALL, "tmux 3.5a"),
            Decision::Call(Why::NotRead)
        );
        assert_eq!(
            decide(BarMode::Auto, LINE, "tmux 3.3a"),
            Decision::Call(Why::OldTmux("tmux 3.3a".into()))
        );
        assert!(matches!(
            decide(BarMode::Auto, LINE, ""),
            Decision::Call(Why::OldTmux(_))
        ));
    }

    #[test]
    fn call_and_push_do_what_they_say_whatever_tmux_reads() {
        assert_eq!(
            decide(BarMode::Call, LINE, "tmux 3.7c"),
            Decision::Call(Why::Configured)
        );
        assert_eq!(
            decide(BarMode::Push, CALL, "tmux 3.3"),
            Decision::Push { read: false }
        );
    }

    #[test]
    fn only_a_tmux_too_old_puts_a_mark_on_the_bar() {
        assert!(health_reason(&Decision::Call(Why::NotRead)).is_none());
        assert!(health_reason(&Decision::Push { read: false }).is_none());
        let old = health_reason(&Decision::Call(Why::OldTmux("tmux 3.2".into())));
        assert!(old.is_some_and(|r| r.contains("tmux 3.2") && r.contains("3.4")));
    }

    fn pane(session: &str, path: &str, visible: bool) -> crate::panes::Pane {
        crate::panes::Pane {
            session: session.into(),
            window_index: 0,
            window_name: String::new(),
            pane_index: 0,
            id: "%1".into(),
            command: "zsh".into(),
            path: path.into(),
            title: String::new(),
            visible,
            activity: 0,
            in_mode: false,
            host: String::new(),
            bell: false,
        }
    }

    #[test]
    fn one_entry_per_attached_session_with_its_active_pane_path() {
        let panes = vec![
            pane("a", "/hidden", false),
            pane("a", "/seen", true),
            pane("b", "/nobody", false),
            pane("c", "/there", true),
        ];
        assert_eq!(
            sessions(&panes),
            vec![
                ("a".to_string(), "/seen".to_string()),
                ("c".to_string(), "/there".to_string())
            ]
        );
    }

    #[test]
    fn changed_sessions_go_in_one_command_sequence() {
        let args = commands(&[("a".into(), "x".into()), ("b c".into(), "y".into())]);
        assert_eq!(
            args,
            [
                "set-option",
                "-t",
                "=a:",
                OPTION,
                "x",
                ";",
                "set-option",
                "-t",
                "=b c:",
                OPTION,
                "y"
            ]
        );
        assert!(commands(&[]).is_empty(), "nothing changed, nothing run");
    }
}
