//! The chunk clock: how long you have been at tmux this sitting, and one sound
//! when that is past the budget.
//!
//! There is no start command, because a timer that has to be started loses to
//! the work it is meant to bound. The daemon watches the clients instead: time
//! counts while one of them has the OS focus and saw a key within
//! `break_after`, whatever session, window or pane it is in. A gap shorter
//! than that counts as work, so reading a long agent reply doesn't stop the
//! clock, and a gap that reaches it is a break that ends the sitting.
//!
//! At budget the sound waits for a boundary, a window switch, a prompt coming
//! back or an agent finishing its turn, because an interruption there costs
//! less than one mid-thought; `boundary_grace` caps the wait. One cue, one
//! snooze, one more cue, then only the bar. Repeating nags are the ones people
//! learn to dismiss. `docs/dev/design-chunk-clock.md` is the spec.
//!
//! Everything that decides is a pure function of a [`Sitting`], a [`Seen`] and
//! the [`Settings`], so the rules are tested without tmux or a clock.

use serde::{Deserialize, Serialize};

/// How often the daemon looks at the clients, in seconds.
///
/// Focus time is the sum of the gaps between looks, so this is its
/// resolution, not its accuracy: a five-second look misses nothing, it only
/// counts the last five seconds a look late. One `list-clients` per look.
pub const POLL_SECS: u64 = 5;

/// The `[chunk]` settings with every duration read into seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub budget: u64,
    pub warn_at: f64,
    pub break_after: u64,
    pub boundary_grace: u64,
    pub snooze: u64,
    pub sound: bool,
    pub banner: bool,
    pub agent_age: bool,
}

impl Settings {
    /// Read from the config. The durations were checked when the file
    /// loaded; a default config never fails them, so the fallbacks are the
    /// shipped values rather than a second opinion.
    pub fn from_config(c: &crate::config::Chunk) -> Self {
        let secs =
            |text: &str, fallback: u64| crate::quiet::parse_duration(text).unwrap_or(fallback);
        Self {
            enabled: c.enabled,
            budget: secs(&c.budget, 50 * 60),
            warn_at: c.warn_at,
            break_after: secs(&c.break_after, 10 * 60),
            boundary_grace: secs(&c.boundary_grace, 5 * 60),
            snooze: secs(&c.snooze, 5 * 60),
            sound: c.sound,
            banner: c.banner,
            agent_age: c.agent_age,
        }
    }
}

/// One sitting, and what the clock has already said about it.
///
/// This is what `state/chunk.toml` holds, so a daemon restart picks the
/// sitting up where it was. The fields after `snooze_at` are the boundary
/// tracking, which means nothing to a new daemon and is not written.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Sitting {
    /// Focus time so far, in seconds.
    pub focus_s: u64,
    /// When focus was last seen, unix seconds; `None` when no sitting runs.
    pub last_active: Option<u64>,
    /// `chunk budget`, which holds until `chunk reset` or a restart.
    #[serde(skip)]
    pub budget_override: Option<u64>,
    /// The budget cue has fired, or a snooze stood in for it.
    pub notified_budget: bool,
    /// When focus time crossed the budget, while the cue waits for a
    /// boundary.
    pub due_since: Option<u64>,
    /// Focus time when `chunk snooze` was asked; the second cue is due
    /// `snooze` after it.
    pub snooze_at: Option<u64>,
    /// The second cue has fired.
    pub notified_snooze: bool,
    /// `chunk close`: nothing counts until you go away and come back.
    pub closed: bool,
    /// The window the focused client was in at the last look.
    #[serde(skip)]
    pub window: String,
    /// What ran in its active pane at the last look.
    #[serde(skip)]
    pub command: String,
}

/// What one look at the clients found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Seen {
    /// One of the attached clients has the OS focus.
    pub focused: bool,
    /// The newest `client_activity` among the focused clients, unix seconds.
    pub activity: u64,
    /// That client's window, `#{window_id}`.
    pub window: String,
    /// What runs in its active pane, `#{pane_current_command}`.
    pub command: String,
    /// The pane's pid, for the agent's process age.
    pub pane_pid: u32,
}

/// What the clock says out loud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// The budget is over.
    Budget,
    /// The snooze is over too.
    Snooze,
}

/// Where a sitting stands, for the bar's colour and the words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    Ok,
    Warn,
    Overdue,
}

impl Sitting {
    /// Whether a sitting is running, which is whether anything is drawn.
    pub fn running(&self) -> bool {
        self.last_active.is_some() && !self.closed
    }

    /// The budget this sitting runs to.
    pub fn budget(&self, s: &Settings) -> u64 {
        self.budget_override.unwrap_or(s.budget)
    }

    /// Ok, warn or overdue.
    pub fn standing(&self, s: &Settings) -> Standing {
        let budget = self.budget(s);
        if self.focus_s >= budget {
            Standing::Overdue
        } else if (self.focus_s as f64) >= budget as f64 * s.warn_at {
            Standing::Warn
        } else {
            Standing::Ok
        }
    }

    /// A new sitting at 0. The budget override survives a break, since only
    /// `chunk reset` and a restart drop it.
    fn begin(&mut self) {
        let budget_override = self.budget_override;
        *self = Sitting {
            budget_override,
            ..Sitting::default()
        };
    }

    /// `chunk reset`: a new sitting at 0, the override and the cues with it.
    pub fn reset(&mut self, now: u64) {
        *self = Sitting {
            last_active: Some(now),
            ..Sitting::default()
        };
    }

    /// `chunk close`: the sitting ends, and nothing counts until you have
    /// been away and come back.
    pub fn close(&mut self) {
        self.begin();
        self.closed = true;
    }

    /// `chunk snooze`: one extension, once per sitting. The answer is what
    /// to tell whoever asked.
    pub fn snooze(&mut self, s: &Settings) -> Result<(), String> {
        if !self.running() {
            return Err("No sitting is running.".to_string());
        }
        if self.snooze_at.is_some() {
            return Err("Already snoozed once this sitting.".to_string());
        }
        if self.focus_s < self.budget(s) {
            return Err(format!("Nothing to snooze yet: {}.", self.words(s)));
        }
        // A cue still waiting for its boundary is the one this replaces.
        self.notified_budget = true;
        self.due_since = None;
        self.snooze_at = Some(self.focus_s);
        Ok(())
    }

    /// The sitting in words, for `chunk status`, `brief` and a screen reader:
    /// `38 min, 12 min for break`, `1 hr 7 min, 17 min passed break time`.
    pub fn words(&self, s: &Settings) -> String {
        let budget = self.budget(s);
        if self.focus_s < budget {
            format!(
                "{}, {} for break",
                spoken(self.focus_s),
                spoken(budget - self.focus_s)
            )
        } else {
            format!(
                "{}, {} passed break time",
                spoken(self.focus_s),
                spoken(self.focus_s - budget)
            )
        }
    }
}

/// Advance a sitting by one look, and say whether it is time for a cue.
///
/// `boundary_at` is the last time the daemon saw an agent finish its turn or
/// stop on a question, from the inbox; the window and the command come from
/// the look itself.
pub fn step(
    sitting: &mut Sitting,
    seen: &Seen,
    now: u64,
    s: &Settings,
    boundary_at: u64,
) -> Option<Cue> {
    let active = seen.focused && now.saturating_sub(seen.activity) < s.break_after;

    // A boundary is a look that differs from the last one in a way that
    // means a thought ended: another window, or a command handing the pane
    // back to its shell.
    let boundary = (!sitting.window.is_empty() && seen.window != sitting.window)
        || (!sitting.command.is_empty()
            && seen.command != sitting.command
            && is_shell(&seen.command))
        || sitting.due_since.is_some_and(|due| boundary_at >= due);
    sitting.window = seen.window.clone();
    sitting.command = seen.command.clone();

    if sitting.closed {
        if !active {
            sitting.closed = false;
        }
        return None;
    }

    if !active {
        // Focused with no key for `break_after` is a break that began at the
        // last key, so it is already long enough. Out of the terminal, the
        // break began at the last look that counted.
        let ended = seen.focused
            || sitting
                .last_active
                .is_some_and(|last| now.saturating_sub(last) >= s.break_after);
        if ended {
            sitting.begin();
        }
        return None;
    }

    match sitting.last_active {
        Some(last) if now.saturating_sub(last) < s.break_after => {
            sitting.focus_s += now.saturating_sub(last);
        }
        Some(_) => sitting.begin(),
        None => {}
    }
    sitting.last_active = Some(now);

    let budget = sitting.budget(s);
    if !sitting.notified_budget && sitting.focus_s >= budget {
        let due = *sitting.due_since.get_or_insert(now);
        if boundary || now.saturating_sub(due) >= s.boundary_grace {
            sitting.notified_budget = true;
            sitting.due_since = None;
            return Some(Cue::Budget);
        }
    }
    if let Some(at) = sitting.snooze_at
        && !sitting.notified_snooze
        && sitting.focus_s >= at + s.snooze
    {
        sitting.notified_snooze = true;
        return Some(Cue::Snooze);
    }
    None
}

/// Whether a pane's command is a shell at its prompt.
fn is_shell(command: &str) -> bool {
    matches!(
        command.trim_start_matches('-'),
        "zsh" | "bash" | "sh" | "fish" | "dash" | "ksh" | "nu" | "elvish"
    )
}

/// Seconds as the bar writes them: `38m`, `1h06m`.
pub fn clock(secs: u64) -> String {
    let m = secs / 60;
    if m < 60 {
        format!("{m}m")
    } else {
        format!("{}h{:02}m", m / 60, m % 60)
    }
}

/// Seconds as they are said: `45 sec`, `12 min`, `1 hr 7 min`.
pub fn spoken(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs} sec");
    }
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    match (h, m) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// The bar's text: `󱫐 38m`, `󱫌 50m+12`, and ` · 1h14m` after either when
/// the pane in front runs an agent and `agent_age` is on.
pub fn bar_text(sitting: &Sitting, s: &Settings, process_age: Option<u64>) -> Option<String> {
    use crate::tmux::icons::{TIMER_ALERT, TIMER_CHECK};
    if !sitting.running() {
        return None;
    }
    let budget = sitting.budget(s);
    let mut out = if sitting.focus_s >= budget {
        format!(
            "{TIMER_ALERT}{}+{}",
            clock(budget),
            (sitting.focus_s - budget) / 60
        )
    } else {
        format!("{TIMER_CHECK}{}", clock(sitting.focus_s))
    };
    if let Some(age) = process_age.filter(|_| s.agent_age) {
        out.push_str(&age_text(age));
    }
    Some(out)
}

/// The agent's age as it follows the time: ` · 1h14m`.
fn age_text(age: u64) -> String {
    format!(" \u{b7} {}", clock(age))
}

/// The colour of the time when the sitting is under `warn_at`.
const OK_COLOUR: &str = crate::tmux::format::FG_GREY89;
/// Past budget: the red the bar uses for something gone wrong.
const OVERDUE_COLOUR: &str = crate::tmux::format::AC_GONE;
/// The process age, which spends nothing and is drawn dim.
const AGE_COLOUR: &str = "colour244";

/// The segment, coloured. The colour is never the only channel: the text
/// itself says `+12` once over, and `chunk status` says it in words.
pub fn segment(sitting: &Sitting, s: &Settings, process_age: Option<u64>, bar_bg: &str) -> String {
    use crate::tmux::format::colored_segment;
    let Some(time) = bar_text(sitting, s, None) else {
        return String::new();
    };
    let colour = match sitting.standing(s) {
        Standing::Ok => OK_COLOUR,
        Standing::Warn => crate::panes::WAITING_COLOUR,
        Standing::Overdue => OVERDUE_COLOUR,
    };
    let mut out = colored_segment(false, colour, bar_bg, &time);
    if let Some(age) = process_age.filter(|_| s.agent_age) {
        out.push_str(&colored_segment(false, AGE_COLOUR, bar_bg, &age_text(age)));
    }
    out
}

/// What `chunk status --json` prints.
#[derive(Serialize, Debug, PartialEq)]
pub struct Status {
    pub running: bool,
    pub focus_s: u64,
    pub budget_s: u64,
    pub overdue_s: u64,
    pub process_age_s: Option<u64>,
    pub state: &'static str,
    pub words: String,
}

/// The sitting as `chunk status --json` reports it.
pub fn status(sitting: &Sitting, s: &Settings, process_age: Option<u64>) -> Status {
    let budget = sitting.budget(s);
    let running = sitting.running();
    Status {
        running,
        focus_s: sitting.focus_s,
        budget_s: budget,
        overdue_s: sitting.focus_s.saturating_sub(budget),
        process_age_s: process_age,
        state: if !running {
            "paused"
        } else {
            match sitting.standing(s) {
                Standing::Ok => "ok",
                Standing::Warn => "warn",
                Standing::Overdue => "overdue",
            }
        },
        words: if running {
            sitting.words(s)
        } else {
            String::new()
        },
    }
}

/// The `list-clients` format [`parse_clients`] reads.
pub const CLIENT_FORMAT: &str =
    "#{client_flags}\t#{client_activity}\t#{window_id}\t#{pane_current_command}\t#{pane_pid}";

/// One look at the clients: the focused one that typed last, or nothing
/// focused.
///
/// `window_active` and `pane_active` belong to the session, so with two
/// clients on one session they would agree anyway; what tells them apart is
/// the OS focus and the last key.
pub fn parse_clients(text: &str) -> Seen {
    let mut best: Option<Seen> = None;
    for line in text.lines() {
        let f: Vec<&str> = line.splitn(5, '\t').collect();
        if f.len() < 5 {
            continue;
        }
        if !f[0].split(',').any(|flag| flag == "focused") {
            continue;
        }
        let seen = Seen {
            focused: true,
            activity: f[1].trim().parse().unwrap_or(0),
            window: f[2].to_string(),
            command: f[3].to_string(),
            pane_pid: f[4].trim().parse().unwrap_or(0),
        };
        if best.as_ref().is_none_or(|b| seen.activity > b.activity) {
            best = Some(seen);
        }
    }
    best.unwrap_or_default()
}

/// Seconds from `ps`'s `etime`, `[[dd-]hh:]mm:ss`.
pub fn parse_etime(text: &str) -> Option<u64> {
    let t = text.trim();
    let (days, rest) = match t.split_once('-') {
        Some((d, rest)) => (d.parse::<u64>().ok()?, rest),
        None => (0, t),
    };
    let parts: Vec<u64> = rest
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let (h, m, s) = match parts.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    Some(days * 86_400 + h * 3600 + m * 60 + s)
}

/// The age of the first child of `parent` in `ps -axo ppid=,etime=`.
pub fn child_age(table: &str, parent: u32) -> Option<u64> {
    table.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let ppid: u32 = words.next()?.parse().ok()?;
        (ppid == parent)
            .then(|| parse_etime(words.next()?))
            .flatten()
    })
}

/// The daemon's half of `chunk`: act on the sitting and say what it is now.
pub fn answer(
    args: &crate::proto::ChunkArgs,
    st: &mut crate::server::state::ServerState,
    now: u64,
) -> anyhow::Result<String> {
    let s = Settings::from_config(&st.config.chunk);
    if !s.enabled {
        return Ok("The chunk clock is off: `[chunk] enabled = true` turns it on.".to_string());
    }
    let changed = match args.action.as_str() {
        "status" => {
            return if args.json {
                Ok(serde_json::to_string(&status(
                    &st.chunk,
                    &s,
                    st.chunk_process_age,
                ))?)
            } else if st.chunk.running() {
                Ok(st.chunk.words(&s))
            } else {
                Ok(String::new())
            };
        }
        "snooze" => {
            if let Err(why) = st.chunk.snooze(&s) {
                return Ok(why);
            }
            format!(
                "Snoozed: one more cue in {}, then only the bar.",
                spoken(s.snooze)
            )
        }
        "reset" => {
            st.chunk.reset(now);
            "A new sitting, at 0.".to_string()
        }
        "close" => {
            st.chunk.close();
            "Sitting closed. The next one starts when you come back from a break.".to_string()
        }
        "budget" => {
            let secs = args
                .budget_secs
                .filter(|secs| *secs > 0)
                .ok_or_else(|| anyhow::anyhow!("budget needs a duration, like 45m"))?;
            st.chunk.budget_override = Some(secs);
            format!("This sitting's budget is {}, until reset.", spoken(secs))
        }
        other => anyhow::bail!("unknown chunk action `{other}`"),
    };
    save(&st.chunk);
    Ok(changed)
}

/// `chunk`: send the action to the daemon and print what it says.
pub async fn run(action: Option<crate::cli::ChunkAction>) -> anyhow::Result<()> {
    use crate::cli::ChunkAction;
    let mut args = crate::proto::ChunkArgs::default();
    match action.unwrap_or(ChunkAction::Status { json: false }) {
        ChunkAction::Status { json } => {
            args.action = "status".into();
            args.json = json;
        }
        ChunkAction::Snooze => args.action = "snooze".into(),
        ChunkAction::Reset => args.action = "reset".into(),
        ChunkAction::Close => args.action = "close".into(),
        ChunkAction::Budget { duration } => {
            args.action = "budget".into();
            args.budget_secs = Some(
                crate::quiet::parse_duration(&duration)
                    .filter(|secs| *secs > 0)
                    .ok_or_else(|| anyhow::anyhow!("not a duration: {duration} (try 45m, 1h)"))?,
            );
        }
    }
    let resp = crate::client::send(crate::proto::Request::build("chunk", &args)).await?;
    if let Some(why) = resp.error {
        anyhow::bail!("{why}");
    }
    if !resp.output.is_empty() {
        println!("{}", resp.output);
    }
    Ok(())
}

/// Where the sitting is kept between daemons.
pub fn state_path() -> Option<std::path::PathBuf> {
    crate::server::state_dir().map(|d| d.join("chunk.toml"))
}

/// The sitting a restart left, if it is still one: a file older than
/// `break_after` was a break, and starts nothing.
pub fn load(now: u64, s: &Settings) -> Sitting {
    state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| toml::from_str::<Sitting>(&text).ok())
        .filter(|sitting| {
            sitting
                .last_active
                .is_some_and(|last| now.saturating_sub(last) < s.break_after)
        })
        .unwrap_or_default()
}

/// Write the sitting down, owner-only. Best effort: a sitting lost to a full
/// disk restarts at 0, which is not worth a health mark.
pub fn save(sitting: &Sitting) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let Some(path) = state_path() else { return };
    let Ok(text) = toml::to_string(sitting) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("toml.tmp");
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if written.is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// The banner command for a body, on this platform.
fn banner_command(body: &str, os: &str) -> Vec<String> {
    match os {
        "macos" => vec![
            "osascript".to_string(),
            "-e".to_string(),
            format!(
                "display notification \"{}\" with title \"tmux-companion\"",
                body.replace('"', "'")
            ),
        ],
        _ => vec![
            "notify-send".to_string(),
            "tmux-companion".to_string(),
            body.to_string(),
        ],
    }
}

/// Say a cue: the sound, quiet hours or not, since it only comes while
/// somebody is at the keyboard; the banner only outside them.
fn say(cue: Cue, sitting: &Sitting, s: &Settings, earcons: &crate::config::Earcons, quiet: bool) {
    if s.sound
        && let Some(command) = crate::earcons::command_regardless(
            earcons,
            crate::config::EarconEvent::Chunk,
            std::env::consts::OS,
        )
    {
        crate::earcons::play(&command);
    }
    if s.banner && !quiet {
        let budget = sitting.budget(s);
        let body = match cue {
            Cue::Budget => format!("{} sitting over", clock(budget)),
            Cue::Snooze => format!(
                "{} passed break time",
                spoken(sitting.focus_s.saturating_sub(budget))
            ),
        };
        crate::earcons::play(&banner_command(&body, std::env::consts::OS));
    }
}

/// The daemon's task: look at the clients every [`POLL_SECS`], keep the
/// sitting, and say a cue when one is due.
pub async fn chunk_loop(
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
) {
    let (settings, earcons) = {
        let st = state.lock().await;
        (
            Settings::from_config(&st.config.chunk),
            st.config.earcons.clone(),
        )
    };
    let restored = load(crate::panes::now_secs(), &settings);
    state.lock().await.chunk = restored;

    let mut saved_minute = u64::MAX;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(POLL_SECS)).await;
        let listing = crate::cli::tmux_capture(&["list-clients", "-F", CLIENT_FORMAT]).await;
        let seen = parse_clients(&listing);
        let now = crate::panes::now_secs();

        // The agent's age costs a `ps`, so it is asked only while an agent is
        // the pane in front.
        let is_agent = {
            let st = state.lock().await;
            crate::panes::is_agent(&seen.command, &st.config.agents.programs)
        };
        let age = if settings.agent_age && is_agent && seen.pane_pid != 0 {
            match tokio::process::Command::new("ps")
                .args(["-axo", "ppid=,etime="])
                .output()
                .await
            {
                Ok(o) => child_age(&String::from_utf8_lossy(&o.stdout), seen.pane_pid),
                Err(_) => None,
            }
        } else {
            None
        };

        let (cue, snapshot, quiet) = {
            let mut st = state.lock().await;
            let boundary_at = st.chunk_boundary_at;
            let cue = step(&mut st.chunk, &seen, now, &settings, boundary_at);
            st.chunk_process_age = age;
            (cue, st.chunk.clone(), st.is_quiet())
        };
        if let Some(cue) = cue {
            say(cue, &snapshot, &settings, &earcons, quiet);
        }
        // Once a minute and on every cue is enough for a restart to resume
        // from; every look would be a write every five seconds.
        let minute = snapshot.focus_s / 60;
        if cue.is_some() || minute != saved_minute {
            save(&snapshot);
            saved_minute = minute;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::from_config(&crate::config::Chunk {
            enabled: true,
            ..Default::default()
        })
    }

    fn at(now: u64, window: &str, command: &str) -> Seen {
        Seen {
            focused: true,
            activity: now,
            window: window.to_string(),
            command: command.to_string(),
            pane_pid: 1,
        }
    }

    /// Looks every five seconds from `from` to `to`, focused and typing,
    /// collecting the cues.
    fn work(sitting: &mut Sitting, from: u64, to: u64, window: &str, command: &str) -> Vec<Cue> {
        let s = settings();
        (from..=to)
            .step_by(POLL_SECS as usize)
            .filter_map(|now| step(sitting, &at(now, window, command), now, &s, 0))
            .collect()
    }

    #[test]
    fn focus_counts_across_sessions_and_windows() {
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 300, "@1", "nvim");
        work(&mut sitting, 305, 600, "@7", "nvim");
        assert_eq!(sitting.focus_s, 600);
    }

    #[test]
    fn a_short_trip_away_counts_and_a_long_one_is_a_break() {
        let s = settings();
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 600, "@1", "nvim");
        // Four minutes in the browser: nothing focused.
        let away = Seen::default();
        for now in (605..840).step_by(5) {
            step(&mut sitting, &away, now, &s, 0);
        }
        work(&mut sitting, 840, 840, "@1", "nvim");
        assert_eq!(sitting.focus_s, 840, "the four minutes count");

        // Ten minutes away from 840 is a break.
        for now in (845..=1440).step_by(5) {
            step(&mut sitting, &away, now, &s, 0);
        }
        assert!(!sitting.running());
        work(&mut sitting, 1445, 1505, "@1", "nvim");
        assert_eq!(sitting.focus_s, 60, "a new sitting at 0");
    }

    #[test]
    fn no_key_for_break_after_ends_the_sitting_even_while_focused() {
        let s = settings();
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 300, "@1", "nvim");
        let idle = at(300, "@1", "nvim");
        for now in (305..=900).step_by(5) {
            step(&mut sitting, &idle, now, &s, 0);
        }
        assert!(!sitting.running());
    }

    #[test]
    fn the_cue_waits_for_a_boundary_at_most_the_grace() {
        let s = settings();
        let mut sitting = Sitting::default();
        // A command running past the budget in one window: no boundary.
        let cues = work(&mut sitting, 0, 3000 + 299, "@1", "cargo");
        assert!(cues.is_empty(), "still inside the grace");
        let cues = work(&mut sitting, 3300, 3300, "@1", "cargo");
        assert_eq!(cues, vec![Cue::Budget], "five minutes late at most");
        assert!(
            work(&mut sitting, 3305, 4000, "@1", "cargo").is_empty(),
            "once"
        );
        let _ = s;
    }

    #[test]
    fn a_prompt_coming_back_is_a_boundary() {
        let mut sitting = Sitting::default();
        assert!(work(&mut sitting, 0, 3010, "@1", "cargo").is_empty());
        assert_eq!(
            work(&mut sitting, 3015, 3015, "@1", "zsh"),
            vec![Cue::Budget]
        );
    }

    #[test]
    fn a_window_switch_is_a_boundary() {
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 3010, "@1", "nvim");
        assert_eq!(
            work(&mut sitting, 3015, 3015, "@2", "nvim"),
            vec![Cue::Budget]
        );
    }

    #[test]
    fn an_agent_finishing_is_a_boundary() {
        let s = settings();
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 3010, "@1", "2.1.289");
        assert_eq!(
            step(&mut sitting, &at(3015, "@1", "2.1.289"), 3015, &s, 3012),
            Some(Cue::Budget)
        );
    }

    #[test]
    fn one_snooze_then_one_more_cue_then_only_the_bar() {
        let s = settings();
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 3015, "@1", "zsh");
        work(&mut sitting, 3020, 3020, "@2", "zsh");
        assert!(sitting.notified_budget);
        assert!(sitting.snooze(&s).is_ok());
        assert!(sitting.snooze(&s).is_err(), "once per sitting");
        let at_snooze = sitting.focus_s;
        let cues = work(&mut sitting, 3025, 3025 + 300 + 60, "@2", "zsh");
        assert_eq!(cues, vec![Cue::Snooze]);
        assert!(sitting.focus_s >= at_snooze + 300);
        assert!(work(&mut sitting, 3400, 5000, "@2", "zsh").is_empty());
    }

    #[test]
    fn a_snooze_before_the_budget_is_refused_with_the_time_left() {
        let s = settings();
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 600, "@1", "nvim");
        assert_eq!(
            sitting.snooze(&s),
            Err("Nothing to snooze yet: 10 min, 40 min for break.".to_string())
        );
    }

    #[test]
    fn close_holds_until_you_go_away_and_come_back() {
        let s = settings();
        let mut sitting = Sitting::default();
        work(&mut sitting, 0, 600, "@1", "nvim");
        sitting.close();
        work(&mut sitting, 605, 900, "@1", "nvim");
        assert!(!sitting.running(), "still at the keyboard: nothing counts");
        step(&mut sitting, &Seen::default(), 905, &s, 0);
        work(&mut sitting, 910, 970, "@1", "nvim");
        assert_eq!(sitting.focus_s, 60);
    }

    #[test]
    fn reset_drops_the_budget_override_and_rearms() {
        let s = settings();
        let mut sitting = Sitting {
            budget_override: Some(60),
            ..Default::default()
        };
        work(&mut sitting, 0, 120, "@1", "zsh");
        work(&mut sitting, 125, 125, "@2", "zsh");
        assert!(sitting.notified_budget);
        sitting.reset(130);
        assert_eq!(sitting.budget(&s), 50 * 60);
        assert!(!sitting.notified_budget);
        assert_eq!(sitting.focus_s, 0);
    }

    #[test]
    fn the_bar_and_the_words() {
        let s = settings();
        let mut sitting = Sitting {
            focus_s: 38 * 60,
            last_active: Some(1),
            ..Default::default()
        };
        assert_eq!(
            bar_text(&sitting, &s, None).as_deref(),
            Some("\u{f1ad0} 38m")
        );
        assert_eq!(sitting.words(&s), "38 min, 12 min for break");
        assert_eq!(sitting.standing(&s), Standing::Ok);
        sitting.focus_s = 41 * 60;
        assert_eq!(sitting.standing(&s), Standing::Warn);
        sitting.focus_s = 67 * 60;
        assert_eq!(
            bar_text(&sitting, &s, None).as_deref(),
            Some("\u{f1acc} 50m+17")
        );
        assert_eq!(sitting.words(&s), "1 hr 7 min, 17 min passed break time");
        assert_eq!(
            bar_text(&sitting, &s, Some(74 * 60)).as_deref(),
            Some("\u{f1acc} 50m+17 \u{b7} 1h14m")
        );
        let without = Settings {
            agent_age: false,
            ..s.clone()
        };
        assert_eq!(
            bar_text(&sitting, &without, Some(74 * 60)).as_deref(),
            Some("\u{f1acc} 50m+17")
        );
        assert_eq!(bar_text(&Sitting::default(), &s, None), None);
    }

    #[test]
    fn spoken_and_clock() {
        assert_eq!(spoken(45), "45 sec");
        assert_eq!(spoken(3600), "1 hr");
        assert_eq!(clock(66 * 60), "1h06m");
    }

    #[test]
    fn the_focused_client_that_typed_last_wins() {
        let seen = parse_clients(
            "attached,UTF-8\t900\t@9\tvim\t3\n\
             attached,focused,UTF-8\t100\t@1\tzsh\t1\n\
             attached,focused,UTF-8\t200\t@2\t2.1.289\t2\n",
        );
        assert_eq!(seen.window, "@2");
        assert_eq!(seen.activity, 200);
        assert_eq!(seen.pane_pid, 2);
        assert!(!parse_clients("attached,UTF-8\t900\t@9\tvim\t3\n").focused);
    }

    #[test]
    fn etime_and_the_child_of_a_pane() {
        assert_eq!(parse_etime("05:07"), Some(307));
        assert_eq!(parse_etime("01:14:00"), Some(74 * 60));
        assert_eq!(parse_etime("2-00:00:01"), Some(2 * 86_400 + 1));
        assert_eq!(parse_etime("x"), None);
        assert_eq!(child_age("  1 99:00\n 42 01:14:00\n", 42), Some(74 * 60));
        assert_eq!(child_age(" 1 01:00\n", 42), None);
    }

    #[test]
    fn a_saved_sitting_round_trips_without_the_boundary_tracking() {
        let sitting = Sitting {
            focus_s: 2280,
            last_active: Some(10),
            notified_budget: true,
            window: "@1".into(),
            budget_override: Some(60),
            ..Default::default()
        };
        let back: Sitting = toml::from_str(&toml::to_string(&sitting).unwrap()).unwrap();
        assert_eq!(back.focus_s, 2280);
        assert!(back.notified_budget);
        assert_eq!(back.window, "");
        assert_eq!(back.budget_override, None);
    }
}
