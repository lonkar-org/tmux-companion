//! the status bar pushed: the daemon renders each attached session's right
//! side and sets it as a session option tmux expands, and a pane option on
//! each pane in another directory, instead of tmux running a process every
//! second to ask.
//!
//! With `[bar] mode = "push"`, or `auto` (the default) when tmux's global
//! `status-right` reads `@tc-right` and tmux is 3.4 or newer, `status-right`
//! is `#{@tc-right}` (or the line below, which falls back to the call), and
//! once a second this loop:
//!
//! 1. takes the shared pane list (`panes::shared`), whose visible panes are
//!    the active pane of each session somebody is attached to, and so the
//!    path each session's git segment is about;
//! 2. renders the right side once for each directory any pane of those
//!    sessions is in, as `status-right` would for that path;
//! 3. sets each session's value on the session, and each pane whose
//!    directory is not its session's gets its own value on the pane
//!    (`set-option -p`), which tmux finds before the session's when that
//!    pane is current. A window or pane switch therefore shows the new
//!    place on tmux's own redraw for the switch, not at the next tick;
//! 4. sends what changed since the last tick in one `tmux` invocation,
//!    `set-option -t =<session>: @tc-right <value> ; set-option -p -t %N
//!    ...`, and nothing at all when nothing did.
//!
//! The cost is one `tmux` process a second for the whole server at most, and
//! the pane list the loops read anyway, against one shell and one client
//! process per attached client per second in call mode. What grows with the
//! server is the git segment, rendered once per directory of an attached
//! session rather than once per session, behind the same git cache; the
//! segments beside it are rendered once a tick.
//!
//! While it pushes the daemon also writes a heartbeat, the global option
//! `@tc-alive`, every [`BEAT`], and the recommended line ([`line`]) shows the
//! pushed value only while that is under [`STALE_SECS`] old. A daemon that
//! stops cleanly (a signal, `restart`, an upgrade) unsets both at once; one
//! that crashes leaves them, the bar shows old figures for at most
//! [`STALE_SECS`], and then the line's call starts a new daemon, which finds
//! the heartbeat left behind, records the unclean stop, and unsets the values
//! it left on every session and pane before it pushes its own.

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

/// The loop's clock: a tick every [`TICK`] from the last one, not [`TICK`]
/// after the work. Sleeping after the work made the period a second plus the
/// render, 1.1-1.2 s and more when a `git status` ran, and a figure held for
/// two seconds whenever one of those landed between tmux's own redraws.
///
/// `Delay` when a tick is missed (a render longer than [`TICK`]): the next
/// one comes a whole [`TICK`] after the late one. `Burst`, the default, would
/// run the missed ones back to back, and two network samples that close
/// share one tick's bytes; `Skip` would wait for the next mark on the
/// original grid, up to a further [`TICK`], which is a longer hold than
/// needed. Nothing here has to stay in phase with anything: a write redraws
/// every client's status line itself.
pub fn ticker() -> tokio::time::Interval {
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick
}

/// Sessions this daemon has set the option in, for clearing on the way out.
static PUSHED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Panes this daemon has set the option on and not unset since, by `%N` id,
/// for clearing on the way out.
static PUSHED_PANES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// What a pane is recorded as carrying after a batch that failed part way:
/// tmux stops a command sequence at its first error (a pane that closed
/// between the listing and the write), so what the rest of it did is not
/// known. Nothing renders to this, so the next tick writes the pane again,
/// or unsets it.
const UNKNOWN: &str = "\0unknown";

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

/// Every pane of an attached session, with the directory its own value is
/// rendered for, or `None` when that is the session's directory and the pane
/// reads the session value instead.
///
/// tmux expands `#{@tc-right}` in the status line for the client's current
/// pane, and finds the option on that pane before its window, and the window
/// before the session. A pane carrying its own value therefore shows it the
/// moment it becomes current, on the redraw tmux does for the switch itself,
/// where the session value waits for the next tick. A pane in the session's
/// own directory would carry the session's value, so it carries nothing and
/// costs no write.
pub fn pane_paths(
    panes: &[crate::panes::Pane],
    sessions: &[(String, String)],
) -> Vec<(String, Option<String>)> {
    panes
        .iter()
        .filter_map(|p| {
            let (_, path) = sessions.iter().find(|(s, _)| s == &p.session)?;
            let own = (p.path != *path).then(|| p.path.clone());
            Some((p.id.clone(), own))
        })
        .collect()
}

/// The pane writes that take what was last written, `last`, to `wanted`:
/// `Some(value)` sets a pane whose value changed, `None` unsets a pane that
/// carries one and should now read the session's. A pane that already holds
/// what it wants is left alone.
pub fn pane_writes(
    wanted: &[(String, Option<String>)],
    last: &HashMap<String, String>,
) -> Vec<(String, Option<String>)> {
    wanted
        .iter()
        .filter(|(id, want)| match want {
            Some(v) => last.get(id) != Some(v),
            None => last.contains_key(id),
        })
        .cloned()
        .collect()
}

/// The `tmux` arguments that set every changed session's value and every
/// pane write, as one command sequence, or nothing when nothing changed.
pub fn commands(changed: &[(String, String)], panes: &[(String, Option<String>)]) -> Vec<String> {
    fn next(args: &mut Vec<String>, cmd: &[&str]) {
        if !args.is_empty() {
            args.push(";".to_string());
        }
        args.extend(cmd.iter().map(|a| a.to_string()));
    }
    let mut args = Vec::new();
    for (session, value) in changed {
        // A trailing colon: set-option takes a pane target, and `=name`
        // alone is refused there as "no such session".
        let target = format!("={session}:");
        next(&mut args, &["set-option", "-t", &target, OPTION, value]);
    }
    for (id, value) in panes {
        match value {
            Some(v) => next(&mut args, &["set-option", "-p", "-t", id, OPTION, v]),
            None => next(&mut args, &["set-option", "-pu", "-t", id, OPTION]),
        }
    }
    args
}

/// The `tmux` arguments that unset the option on `sessions` and `panes`.
pub fn unset_commands(sessions: &[String], panes: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    let targets = sessions
        .iter()
        .map(|s| ("-u", format!("={s}:")))
        .chain(panes.iter().map(|p| ("-pu", p.clone())));
    for (flag, target) in targets {
        if !args.is_empty() {
            args.push(";".to_string());
        }
        args.extend([
            "set-option".to_string(),
            flag.to_string(),
            "-t".to_string(),
            target,
            OPTION.to_string(),
        ]);
    }
    args
}

/// The sessions and pane ids `list-panes -a -F "#{session_name}\t#{pane_id}"`
/// printed, so an unset never names one that is gone: tmux stops a command
/// sequence at the first target it cannot find.
pub fn live(listing: &str) -> (Vec<String>, Vec<String>) {
    let mut sessions: Vec<String> = Vec::new();
    let mut panes = Vec::new();
    for line in listing.lines() {
        let Some((s, p)) = line.rsplit_once('\t') else {
            continue;
        };
        if !sessions.iter().any(|x| x == s) {
            sessions.push(s.to_string());
        }
        panes.push(p.to_string());
    }
    (sessions, panes)
}

/// The listing [`live`] reads.
const LIVE_FORMAT: &str = "#{session_name}\t#{pane_id}";

/// Unset the option on every session and pane of the server: what a daemon
/// that stopped uncleanly left behind. A pane value left there would win over
/// the session value this daemon writes, and this daemon leaves the panes in
/// their session's directory unwritten, so it would show for good.
async fn clear_left_behind() {
    let Ok(out) = crate::tmux::command()
        .args(["list-panes", "-a", "-F", LIVE_FORMAT])
        .output()
        .await
    else {
        return;
    };
    let (sessions, panes) = live(&String::from_utf8_lossy(&out.stdout));
    let args = unset_commands(&sessions, &panes);
    if args.is_empty() {
        return;
    }
    let _ = crate::tmux::command()
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
}

/// The right side for `path`: the git segment once a tick however many panes
/// are in that directory, beside the parts that do not depend on it, which
/// are rendered once a tick for every directory.
async fn render_for(
    path: &str,
    parts: &crate::server::RightParts,
    state: &Arc<Mutex<ServerState>>,
    memo: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    if let Some(v) = memo.get(path) {
        return v.clone();
    }
    let value = match serde_json::from_value::<crate::proto::StatusRightArgs>(
        serde_json::json!({ "path": path }),
    ) {
        Ok(args) => Some(parts.with_git(&crate::server::git_part(&args, state).await)),
        Err(_) => None,
    };
    memo.insert(path.to_string(), value.clone());
    value
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
    let mut last_panes: HashMap<String, String> = HashMap::new();
    let mut checked: Option<std::time::Instant> = None;
    let mut active = false;
    let mut beat: Option<std::time::Instant> = None;
    let mut looked_for_crash = false;
    let mut tick = ticker();
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
                tick.tick().await;
                continue;
            };
            let status_right = String::from_utf8_lossy(&answer.stdout).into_owned();
            // Once, on the first answer: a heartbeat already there is one the
            // last daemon left behind, and so are its values.
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
                    clear_left_behind().await;
                }
            }
            let decision = decide(mode, &status_right, &tmux_v);
            let now = matches!(decision, Decision::Push { .. });
            if active && !now {
                unset_all();
                last.clear();
                last_panes.clear();
            }
            active = now;
            state.lock().await.bar_note = health_reason(&decision);
            checked = Some(std::time::Instant::now());
        }
        tick.tick().await;
        if !active {
            continue;
        }
        let panes = crate::panes::shared(&state, TICK).await;
        let attached = sessions(&panes);
        let mut memo: HashMap<String, Option<String>> = HashMap::new();
        // Once a tick, and so one network sample a tick: two would split the
        // rate's window between them.
        let parts = crate::server::right_parts(&state).await;
        let mut changed = Vec::new();
        for (session, path) in &attached {
            let Some(value) = render_for(path, &parts, &state, &mut memo).await else {
                continue;
            };
            if last.get(session) != Some(&value) {
                last.insert(session.clone(), value.clone());
                changed.push((session.clone(), value));
            }
        }
        let mut wanted = Vec::new();
        for (id, path) in pane_paths(&panes, &attached) {
            match path {
                None => wanted.push((id, None)),
                Some(path) => {
                    if let Some(value) = render_for(&path, &parts, &state, &mut memo).await {
                        wanted.push((id, Some(value)));
                    }
                }
            }
        }
        // A pane that closed took its option with it.
        last_panes.retain(|id, _| panes.iter().any(|p| &p.id == id));
        let writes = pane_writes(&wanted, &last_panes);
        for (id, value) in &writes {
            match value {
                Some(v) => last_panes.insert(id.clone(), v.clone()),
                None => last_panes.remove(id),
            };
        }
        let beat_due = beat.is_none_or(|t| t.elapsed() >= BEAT);
        if changed.is_empty() && writes.is_empty() && !beat_due {
            if let Ok(mut pushed) = PUSHED_PANES.lock() {
                *pushed = last_panes.keys().cloned().collect();
            }
            continue;
        }
        if let Ok(mut pushed) = PUSHED.lock() {
            for (s, _) in &changed {
                if !pushed.contains(s) {
                    pushed.push(s.clone());
                }
            }
        }
        // Every pane written to counts as possibly set until a batch says
        // otherwise, so a stop in the middle of this one still clears it.
        if let Ok(mut pushed) = PUSHED_PANES.lock() {
            let mut ids: Vec<String> = last_panes.keys().cloned().collect();
            ids.extend(writes.iter().map(|(id, _)| id.clone()));
            ids.sort();
            ids.dedup();
            *pushed = ids;
        }
        // The heartbeat first, so a pane that closed before its write, which
        // ends the sequence there, does not take the heartbeat with it. No
        // `refresh-client -S` after it: a `set-option` already redraws every
        // client's status line (3.4 and 3.7c), and with no client attached
        // the refresh fails with `no current client`, which would fail the
        // batch and have every value written again on the next tick.
        let mut args = Vec::new();
        if beat_due {
            args.extend([
                "set-option".to_string(),
                "-g".to_string(),
                ALIVE.to_string(),
                crate::panes::now_secs().to_string(),
            ]);
            beat = Some(std::time::Instant::now());
        }
        let rest = commands(&changed, &writes);
        if !args.is_empty() && !rest.is_empty() {
            args.push(";".to_string());
        }
        args.extend(rest);
        // Under the write lock, and not at all once the daemon is stopping:
        // a write that landed after `clear` would put back a value nobody
        // will update or unset.
        let ok = tokio::task::spawn_blocking(move || {
            let _writing = WRITE.lock();
            if STOPPING.load(std::sync::atomic::Ordering::SeqCst) {
                return true;
            }
            crate::tmux::command_sync()
                .args(&args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
        .await
        .unwrap_or(false);
        if !ok {
            // Somewhere in the sequence tmux stopped; what came after it was
            // not written. Forget it all, and the next tick writes it again.
            for (s, _) in &changed {
                last.remove(s);
            }
            for (id, _) in &writes {
                last_panes.insert(id.clone(), UNKNOWN.to_string());
            }
        }
    }
}

/// Held while the loop's tmux invocation runs, so [`clear`] waits for one in
/// flight rather than racing it.
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set by [`clear`]: the daemon is on its way out and the loop writes nothing
/// more. Seen once in the e2e suite before it existed: a tick's write landed
/// after the shutdown's unset and left pane values behind.
static STOPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Unset the option everywhere this daemon set it, sessions and panes, so the
/// bar falls back to the call rather than showing a figure nobody is
/// updating, and stop the loop writing again. For the way out: a signal, a
/// shutdown. Synchronous: it runs on the way out of a signal handler.
pub fn clear() {
    STOPPING.store(true, std::sync::atomic::Ordering::SeqCst);
    let _writing = WRITE.lock();
    unset_all();
}

/// [`clear`] without stopping the loop: for `auto` going back to the call.
fn unset_all() {
    let sessions = PUSHED.lock().map(|p| p.clone()).unwrap_or_default();
    let panes = PUSHED_PANES.lock().map(|p| p.clone()).unwrap_or_default();
    // The heartbeat first: gone, the line falls back to the call at once, and
    // the next daemon does not take this stop for a crash.
    let mut args: Vec<String> = ["set-option", "-gu", ALIVE]
        .iter()
        .map(|a| a.to_string())
        .collect();
    if !sessions.is_empty() || !panes.is_empty() {
        // Only the ones still there: one that is gone would end the sequence.
        let listing = crate::tmux::command_sync()
            .args(["list-panes", "-a", "-F", LIVE_FORMAT])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let (live_sessions, live_panes) = live(&listing);
        let sessions: Vec<String> = sessions
            .into_iter()
            .filter(|s| live_sessions.contains(s))
            .collect();
        let panes: Vec<String> = panes
            .into_iter()
            .filter(|p| live_panes.contains(p))
            .collect();
        let rest = unset_commands(&sessions, &panes);
        if !rest.is_empty() {
            args.push(";".to_string());
            args.extend(rest);
        }
    }
    let _ = crate::tmux::command_sync()
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if let Ok(mut p) = PUSHED_PANES.lock() {
        p.clear();
    }
    if let Ok(mut p) = PUSHED.lock() {
        p.clear();
    }
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
            session_id: String::new(),
            accent: String::new(),
            pid: 0,
            identity: None,
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
        let args = commands(&[("a".into(), "x".into()), ("b c".into(), "y".into())], &[]);
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
        assert!(
            commands(&[], &[]).is_empty(),
            "nothing changed, nothing run"
        );
    }

    fn pane_at(session: &str, id: &str, path: &str, visible: bool) -> crate::panes::Pane {
        crate::panes::Pane {
            id: id.into(),
            ..pane(session, path, visible)
        }
    }

    #[test]
    fn a_pane_carries_its_own_value_only_where_its_directory_differs() {
        let panes = vec![
            pane_at("a", "%1", "/repo", true),
            pane_at("a", "%2", "/repo", false),
            pane_at("a", "%3", "/elsewhere", false),
            pane_at("b", "%4", "/nobody/attached", false),
        ];
        let attached = sessions(&panes);
        assert_eq!(
            pane_paths(&panes, &attached),
            vec![
                ("%1".to_string(), None),
                ("%2".to_string(), None),
                ("%3".to_string(), Some("/elsewhere".to_string())),
            ],
            "the current pane and its neighbour read the session's; b has no client"
        );
    }

    #[test]
    fn only_a_changed_pane_is_written_and_one_back_home_is_unset() {
        let last: HashMap<String, String> = [
            ("%1".to_string(), "same".to_string()),
            ("%2".to_string(), "old".to_string()),
            ("%3".to_string(), "was its own".to_string()),
            ("%9".to_string(), UNKNOWN.to_string()),
        ]
        .into();
        let wanted = vec![
            ("%1".to_string(), Some("same".to_string())),
            ("%2".to_string(), Some("new".to_string())),
            ("%3".to_string(), None),
            ("%4".to_string(), None),
            ("%5".to_string(), Some("first".to_string())),
            ("%9".to_string(), None),
        ];
        assert_eq!(
            pane_writes(&wanted, &last),
            vec![
                ("%2".to_string(), Some("new".to_string())),
                ("%3".to_string(), None),
                ("%5".to_string(), Some("first".to_string())),
                ("%9".to_string(), None),
            ]
        );
    }

    #[test]
    fn pane_writes_join_the_sessions_in_the_same_sequence() {
        let args = commands(
            &[("a".into(), "x".into())],
            &[("%3".into(), Some("y".into())), ("%4".into(), None)],
        );
        assert_eq!(
            args.join(" "),
            format!(
                "set-option -t =a: {OPTION} x ; set-option -p -t %3 {OPTION} y ; \
                 set-option -pu -t %4 {OPTION}"
            )
        );
        assert_eq!(
            commands(&[], &[("%3".into(), Some("y".into()))]).join(" "),
            format!("set-option -p -t %3 {OPTION} y"),
            "no leading separator"
        );
    }

    #[test]
    fn clearing_names_only_what_is_still_there() {
        let (sessions, panes) = live("a\t%1\na\t%2\nb c\t%7\n\n");
        assert_eq!(sessions, ["a", "b c"]);
        assert_eq!(panes, ["%1", "%2", "%7"]);
        assert_eq!(
            unset_commands(&["a".into()], &["%2".into()]).join(" "),
            format!("set-option -u -t =a: {OPTION} ; set-option -pu -t %2 {OPTION}")
        );
        assert!(unset_commands(&[], &[]).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_period_is_a_tick_whatever_the_render_takes() {
        let mut tick = ticker();
        tick.tick().await;
        let start = tokio::time::Instant::now();
        // A render of 300 ms, then the next tick: a second after the last,
        // not a second after the render.
        tokio::time::advance(Duration::from_millis(300)).await;
        tick.tick().await;
        assert_eq!(start.elapsed(), TICK);
        // A render longer than a tick: the next comes at once, and the one
        // after a whole tick later, never two back to back.
        tokio::time::advance(Duration::from_millis(1500)).await;
        tick.tick().await;
        let late = tokio::time::Instant::now();
        assert_eq!(late - start, TICK + Duration::from_millis(1500));
        tick.tick().await;
        assert_eq!(late.elapsed(), TICK);
    }
}
