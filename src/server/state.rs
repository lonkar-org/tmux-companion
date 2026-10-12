use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use crate::{
    cache::TtlMap,
    config::{Config, GlyphMap},
    segments::agents::AgentsSample,
    segments::git::GitStatus,
    segments::health::HealthSample,
    segments::network::NetSample,
};

/// Default freshness window for a cached `git status`, overridable per request
/// with `--ttl`.  Five seconds: long enough that a one-second status bar is
/// served from memory four times out of five, short enough that a commit made
/// in the pane shows up before the user wonders why it has not.
pub const DEFAULT_GST_TTL: Duration = Duration::from_secs(5);

/// How long to trust "this path is / is not inside a work tree".
///
/// A directory's repo-ness effectively never changes, so this could be cached
/// forever — except that `git init` in a directory already on the bar would
/// then be misremembered until the server restarts.  Five minutes keeps the
/// fork off the hot path (it is the largest remaining per-call cost) while
/// bounding how long a fresh `git init` stays invisible.
pub const REPO_CHECK_TTL: Duration = Duration::from_secs(300);

/// Battery is sampled at most this often; IOKit is expensive and the reading
/// does not move fast enough to matter.
pub const BATTERY_TTL: Duration = Duration::from_secs(30);

/// How long a render that shows the battery charging is trusted.
///
/// It used not to be cached at all, so on mains power the bar asked the
/// battery every second, through IOKit on the blocking pool, for a figure
/// that moves one percent in a minute or so. Ten seconds keeps the charging
/// figure close to live and drops nine reads in ten.
pub const BATTERY_CHARGING_TTL: Duration = Duration::from_secs(10);

/// One directory source's kept answer.
#[derive(Debug, Clone)]
pub struct DirsAnswer {
    /// The paths, one a line, as the source printed them.
    pub paths: String,
    /// When they were read.
    pub read_at: Instant,
    /// The watched file's modification time when they were read.
    pub stamp: Option<SystemTime>,
}

/// Everything the server keeps between requests: the bandwidth previous
/// sample, the directory aliases, and the three caches.
///
/// Dies with the process, which is the intended lifetime: one cold `git
/// status` after a restart costs 51 ms, once.
pub struct ServerState {
    /// The configuration this daemon started with.
    ///
    /// Parsed once, never re-read per request: the daemon exists partly to stop
    /// repeated reads of config files, so reading one per render would be a
    /// poor joke. `tmux-companion reload` is what picks up an edit.
    pub config: Config,
    /// The glyph substitutions the config asks for, resolved once at start.
    ///
    /// Behind an `Arc` so the response path clones a pointer rather than a
    /// table on every request.
    pub glyphs: std::sync::Arc<GlyphMap>,
    /// The branch-prefix table the git segment draws with, and what the
    /// glyph-name lookup has picked so far. See `branch_icons`.
    pub branch_icons: crate::branch_icons::BranchIcons,
    /// Previous cumulative rx/tx counters, for the bandwidth delta.
    pub net_previous: Option<NetSample>,
    /// The last bandwidth string rendered, replayed when two samples arrive too
    /// close together to divide by (see `segments::network`).
    pub net_last_render: String,
    /// Directory-to-label overrides for the window segment.
    pub dir_aliases: HashMap<PathBuf, String>,
    /// Cached battery render with the time it was computed.
    pub battery_cache: Option<(String, Instant)>,
    /// The last agent count, with the time the pane list was read.
    ///
    /// The count rather than the render, because the render depends on the
    /// bar background and the count is the part that costs a tmux call.
    pub agents_cache: Option<(AgentsSample, Instant)>,
    /// The last `list-panes -a`, and when it was read: one listing for the
    /// inbox, notify and journal loops and the agents segment, each taking
    /// it when it is younger than its own interval. See `panes::shared`.
    pub pane_list: Option<(Vec<crate::panes::Pane>, Instant)>,
    /// The session list read in the same tmux call as `pane_list`, mirror
    /// sessions left out, with when it was read as an instant and in unix
    /// seconds. What a link's `sessions` answers from.
    pub session_list: Option<(Vec<crate::link::Session>, Instant, u64)>,
    /// The mirror sessions this machine opened, from the same listing.
    pub mirrors: Vec<crate::link::Mirror>,
    /// The grouped sessions another machine's mirrors attached to, from the
    /// same listing: the push loop leaves them alone, since their bar is off.
    pub grouped: Vec<String>,
    /// The grouped sessions with a client attached, each with the machine
    /// its `@tc-mirror` names: the ones a bridge's `watch` pushes segments
    /// for.
    pub grouped_shown: Vec<(String, String)>,
    /// Bumped whenever something a bridge's `watch` pushes may have changed:
    /// the session list, the inbox, or, while a grouped session has a
    /// client, any new listing, since its segments ride on that. A `watch`
    /// waits on this rather than on a timer of its own.
    pub watch_gen: tokio::sync::watch::Sender<u64>,
    /// Every host this daemon links to, and what each last said.
    pub links: crate::link::manager::Links,
    /// Directory sources' answers, by what was asked (`dirs`): the paths,
    /// when they were read, and the watched file's modification time then.
    /// See `dirsource::still_good` for when one is given again.
    pub dirs_cache: HashMap<String, DirsAnswer>,
    /// Why the bar cannot be pushed, when that is worth the health mark: a
    /// tmux too old under `[bar] mode = "auto"`. See `push::health_reason`.
    pub bar_note: Option<String>,
    /// Unclean stops of the daemon before this one, as the health mark says
    /// them; see `push::note_unclean_stop`. Kept for the failure window or
    /// until `health ack`, like a failure, but not worded as one.
    pub unclean: Vec<(String, Instant)>,
    /// When this daemon came up, for telling a file edited since from one
    /// edited before.
    pub started_at: std::time::SystemTime,
    /// When config.toml was last taken in whole: the start, or a hosts
    /// reload that found nothing but `[[hosts]]` had changed, which is the
    /// daemon's own write from `hosts add --save` or `hosts forget`. The
    /// health mark's "config.toml changed" is measured from here.
    pub config_read_at: std::time::SystemTime,
    /// What failed and when, for the health mark, under the name of the
    /// timer or segment it came from: one entry each, so a timer that works
    /// again can take its own failure back and leave the others standing.
    pub failures: std::collections::BTreeMap<String, (String, Instant)>,
    /// The last health check with the time it ran.
    pub health_cache: Option<(HealthSample, Instant)>,
    /// Since when the network has been gone, in unix seconds, as the online
    /// task last found it; none is online, or nobody asking.
    pub offline_since: Option<u64>,
    /// The agents that have stopped, keyed by pane id, kept by the inbox task.
    pub inbox: std::collections::HashMap<String, crate::inbox::Entry>,
    /// Which of them have been nudged about, so each is nudged once.
    pub nudged: std::collections::HashSet<String>,
    /// When quiet hours end, in unix seconds; none or past means not quiet.
    pub quiet_until: Option<u64>,
    /// The Focus that is on, by name, from `quiet --focus` or the
    /// `[quiet] follow_focus` watcher (`quiet::focus_loop` on macOS,
    /// `dnd::follow` on Linux); quiet for as long as it is set.
    pub focus: Option<String>,
    /// The chunk clock's sitting, kept by `chunk::chunk_loop`.
    pub chunk: crate::chunk::Sitting,
    /// When the inbox last saw an agent finish its turn or stop on a
    /// question, which is a boundary the chunk clock's cue may wait for.
    pub chunk_boundary_at: u64,
    /// How long the agent in the focused pane has been running, from the
    /// last look; `None` when that pane is no agent.
    pub chunk_process_age: Option<u64>,
    /// What each agent last said about itself, keyed by pane id, from
    /// `tmux-companion agent` run out of its hooks. Dropped when the pane
    /// goes or stops running an agent.
    pub reports: crate::panes::Reports,
    /// Whether any agent has reported since this daemon started.
    ///
    /// Kept apart from `reports`, which forgets a pane once it closes: the
    /// question `setup` asks is whether the hooks are wired at all, and one
    /// report ever is the answer.
    pub agent_reported: bool,
    /// Parsed `git status`, keyed by canonicalized repository path.
    git_cache: TtlMap<PathBuf, GitStatus>,
    /// One lock per repository a `git status` is being run for, so two bar
    /// requests that miss the cache together run git once: the second waits
    /// for the first and reads what it stored. Bounded by the repositories
    /// the bar has shown, the same as `git_cache`.
    git_flights: HashMap<PathBuf, std::sync::Arc<tokio::sync::Mutex<()>>>,
    /// Whether a path is inside a git work tree, keyed by canonicalized path.
    repo_check: TtlMap<PathBuf, bool>,
    /// Repository roots the bar has drawn, for the autofetch task.
    ///
    /// Separate from `repo_check`, which also holds every path that turned out
    /// not to be a repository and is keyed by the pane's directory rather than
    /// by the root. This one holds roots, so two panes in the same tree are one
    /// fetch.
    seen_repos: TtlMap<PathBuf, ()>,
    /// Key bindings, with the config mtime they were built from.
    ///
    /// Not a `TtlMap`: bindings change when the config is sourced and never
    /// otherwise, so the freshness question is "is the file newer than these
    /// rows" rather than "how old are they". That is also what deletes the
    /// cache file, the `--build` flag and the `--refresh` flag the zsh version
    /// needed.
    keys: Option<(Vec<crate::keys::KeyRow>, Option<std::time::SystemTime>)>,
}

/// Every method on `ServerState` is synchronous by design: the state lives
/// behind a `tokio::sync::Mutex`, and keeping the methods sync makes it
/// impossible to hold the guard across an `.await`.  See the lock-discipline
/// invariant in `CLAUDE.md`.
impl ServerState {
    /// Fresh state with empty caches, the defaults, and the aliases loaded from
    /// disk.
    pub fn new() -> Self {
        Self::with_config(Config::default())
    }

    /// Fresh state carrying a config somebody already parsed.
    pub fn with_config(config: Config) -> Self {
        let config_aliases = config.dirs.aliases.clone();
        let glyphs = std::sync::Arc::new(GlyphMap::new(&config.glyphs));
        let branch_icons = crate::branch_icons::BranchIcons::new(&config.git, config.glyphs.preset);
        Self {
            config,
            glyphs,
            branch_icons,
            net_previous: None,
            net_last_render: String::new(),
            dir_aliases: if config_aliases.is_empty() {
                load_dir_aliases()
            } else {
                config_aliases
            },
            battery_cache: None,
            agents_cache: None,
            pane_list: None,
            session_list: None,
            mirrors: Vec::new(),
            grouped: Vec::new(),
            grouped_shown: Vec::new(),
            watch_gen: tokio::sync::watch::Sender::new(0),
            links: crate::link::manager::Links::default(),
            dirs_cache: HashMap::new(),
            bar_note: None,
            unclean: Vec::new(),
            started_at: std::time::SystemTime::now(),
            config_read_at: std::time::SystemTime::now(),
            failures: std::collections::BTreeMap::new(),
            health_cache: None,
            offline_since: None,
            inbox: std::collections::HashMap::new(),
            nudged: std::collections::HashSet::new(),
            quiet_until: None,
            focus: None,
            chunk: crate::chunk::Sitting::default(),
            chunk_boundary_at: 0,
            chunk_process_age: None,
            reports: crate::panes::Reports::new(),
            agent_reported: false,
            git_cache: TtlMap::new(),
            git_flights: HashMap::new(),
            repo_check: TtlMap::new(),
            seen_repos: TtlMap::new(),
            keys: None,
        }
    }

    // ── git status ───────────────────────────────────────────────────────────

    /// The configured `git status` freshness window.
    pub fn git_ttl(&self) -> Duration {
        secs(self.config.git.ttl_secs, DEFAULT_GST_TTL)
    }

    /// The configured is-inside-work-tree freshness window.
    /// Note that the bar drew a repository, so the autofetch task knows about
    /// it.
    ///
    /// Synchronous like every other method here: it is one map write, and the
    /// rule that no `ServerState` method can suspend is what makes the
    /// deadlock in the combined `status-right` handler impossible to write.
    pub fn note_repo(&mut self, root: PathBuf, remember: Duration) {
        self.seen_repos.insert(root, (), remember);
    }

    /// The repository roots drawn within `remember`.
    pub fn repos_to_fetch(&self, remember: Duration) -> Vec<PathBuf> {
        self.seen_repos.fresh_keys(remember)
    }

    /// How long "this path is inside a work tree" stays trusted.
    pub fn repo_check_ttl(&self) -> Duration {
        secs(self.config.git.repo_check_ttl_secs, REPO_CHECK_TTL)
    }

    /// The configured battery freshness window.
    pub fn battery_ttl(&self) -> Duration {
        secs(self.config.battery.ttl_secs, BATTERY_TTL)
    }

    /// A parsed status for `path`, if one was stored less than `ttl` ago.
    pub fn git_cached(&self, path: &PathBuf, ttl: Duration) -> Option<GitStatus> {
        self.git_cache.get(path, ttl)
    }

    /// The lock a `git status` for `path` is run under; see `git_flights`.
    pub fn git_flight(&mut self, path: &std::path::Path) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        std::sync::Arc::clone(self.git_flights.entry(path.to_path_buf()).or_default())
    }

    /// Store a freshly parsed status.
    pub fn git_store(&mut self, path: PathBuf, status: GitStatus, ttl: Duration) {
        self.git_cache.insert(path, status, ttl);
    }

    // ── is-inside-work-tree ──────────────────────────────────────────────────

    /// Whether `path` is inside a work tree, if that was answered recently.
    pub fn repo_cached(&self, path: &PathBuf) -> Option<bool> {
        self.repo_check.get(path, self.repo_check_ttl())
    }

    /// Remember whether `path` is inside a work tree, including a no.
    pub fn repo_store(&mut self, path: PathBuf, inside: bool) {
        let ttl = self.repo_check_ttl();
        self.repo_check.insert(path, inside, ttl);
    }

    // ── battery ──────────────────────────────────────────────────────────────

    // ── key bindings ─────────────────────────────────────────────────────────

    /// The cached rows, if they were built from the config as it is now.
    pub fn keys_cached(
        &self,
        mtime: Option<std::time::SystemTime>,
    ) -> Option<Vec<crate::keys::KeyRow>> {
        match &self.keys {
            Some((rows, built_from)) if *built_from == mtime => Some(rows.clone()),
            _ => None,
        }
    }

    /// Store rows against the config mtime they were built from.
    pub fn keys_store(
        &mut self,
        rows: Vec<crate::keys::KeyRow>,
        mtime: Option<std::time::SystemTime>,
    ) {
        self.keys = Some((rows, mtime));
    }

    /// The last battery render, if it is still fresh.
    pub fn battery_cached(&self) -> Option<String> {
        self.battery_cache
            .as_ref()
            .filter(|(s, t)| {
                let ttl = if s.contains(crate::segments::battery::CHARGING_ICON) {
                    self.battery_ttl().min(BATTERY_CHARGING_TTL)
                } else {
                    self.battery_ttl()
                };
                t.elapsed() < ttl
            })
            .map(|(s, _)| s.clone())
    }

    /// Store a battery render with the time it was computed.
    pub fn battery_store(&mut self, rendered: String) {
        self.battery_cache = Some((rendered, Instant::now()));
    }

    // ── the pane list ────────────────────────────────────────────────────────

    /// The shared pane list, if it was read less than `max_age` ago.
    pub fn panes_fresh(&self, max_age: Duration) -> Option<Vec<crate::panes::Pane>> {
        self.pane_list
            .as_ref()
            .filter(|(_, at)| at.elapsed() < max_age)
            .map(|(p, _)| p.clone())
    }

    /// A directory source's kept answer, when it can be given again with the
    /// watched file's modification time now being `now`.
    pub fn dirs_fresh(&self, key: &str, now: Option<SystemTime>, watched: bool) -> Option<String> {
        let a = self.dirs_cache.get(key)?;
        crate::dirsource::still_good(a.read_at.elapsed(), a.stamp, now, watched)
            .then(|| a.paths.clone())
    }

    /// Keep a directory source's answer just read.
    pub fn dirs_store(&mut self, key: String, paths: String, stamp: Option<SystemTime>) {
        self.dirs_cache.insert(
            key,
            DirsAnswer {
                paths,
                read_at: Instant::now(),
                stamp,
            },
        );
    }

    /// Keep a pane list just read, for the next reader.
    pub fn panes_store(&mut self, panes: Vec<crate::panes::Pane>) {
        self.pane_list = Some((panes, Instant::now()));
    }

    /// The session list read with the pane list, and when (unix seconds), if
    /// it was read less than `max_age` ago.
    pub fn sessions_fresh(&self, max_age: Duration) -> Option<(Vec<crate::link::Session>, u64)> {
        self.session_list
            .as_ref()
            .filter(|(_, at, _)| at.elapsed() < max_age)
            .map(|(s, _, when)| (s.clone(), *when))
    }

    /// Keep a session listing just read with the pane list.
    pub fn sessions_store(&mut self, listing: crate::link::Listing) {
        let changed = self
            .session_list
            .as_ref()
            .is_none_or(|(old, _, _)| crate::link::sessions_differ(old, &listing.sessions))
            || self.grouped_shown != listing.shown;
        let shown = !listing.shown.is_empty();
        self.session_list = Some((listing.sessions, Instant::now(), crate::panes::now_secs()));
        self.mirrors = listing.mirrors;
        self.grouped = listing.grouped;
        self.grouped_shown = listing.shown;
        if changed || shown {
            self.watch_bump();
        }
    }

    /// Tell every waiting `watch` that something it pushes may have changed.
    pub fn watch_bump(&mut self) {
        self.watch_gen.send_modify(|g| *g = g.wrapping_add(1));
    }

    /// Keep the inbox a pass just worked out, and wake a waiting `watch`
    /// when it differs from the one held.
    pub fn inbox_set(&mut self, inbox: HashMap<String, crate::inbox::Entry>) {
        let same = inbox.len() == self.inbox.len()
            && inbox.iter().all(|(id, e)| {
                self.inbox.get(id).is_some_and(|o| {
                    o.since == e.since
                        && o.state == e.state
                        && o.question == e.question
                        && o.at == e.at
                })
            });
        self.inbox = inbox;
        if !same {
            self.watch_bump();
        }
    }

    // ── agents ───────────────────────────────────────────────────────────────

    /// Whether any configured segment is the agent count.
    ///
    /// When none is, the pane list is never read: the segment is off the
    /// default side precisely so a machine without agents pays nothing for it.
    pub fn agents_wanted(&self) -> bool {
        self.config
            .status
            .right
            .draws(crate::config::SegmentName::Agents)
    }

    /// How long one read of the pane list is trusted.
    pub fn agents_interval(&self) -> Duration {
        Duration::from_secs(self.config.agents.interval_secs)
    }

    /// How long the bar trusts a count, or a pane list, the inbox loop left:
    /// its interval and a second. Exactly the interval expired a beat before
    /// the loop's next read had landed (the loop sleeps the interval, then
    /// lists), so the bar ran a `list-panes` of its own every so often.
    pub fn agents_trust(&self) -> Duration {
        self.agents_interval() + Duration::from_secs(1)
    }

    /// The last count, if it is still fresh.
    pub fn agents_cached(&self) -> Option<AgentsSample> {
        self.agents_cache
            .as_ref()
            .filter(|(_, t)| t.elapsed() < self.agents_trust())
            .map(|(s, _)| *s)
    }

    /// Store a count with the time the list was read.
    pub fn agents_store(&mut self, sample: AgentsSample) {
        self.agents_cache = Some((sample, Instant::now()));
    }

    /// Keep only the reports about panes still running an agent.
    ///
    /// A report is trusted for as long as the pane it names runs an agent,
    /// so this is the one place it goes: when the pane vanishes, or a shell
    /// takes its place. Called by whichever task read the pane list last.
    pub fn retain_reports(&mut self, panes: &[crate::panes::Pane]) {
        let programs = &self.config.agents.programs;
        let live: std::collections::HashSet<&str> = panes
            .iter()
            .filter(|p| crate::panes::is_agent(p.name(), programs))
            .map(|p| p.id.as_str())
            .collect();
        self.reports.retain(|id, _| live.contains(id.as_str()));
    }

    /// Whether quiet hours are on right now: a timer, a daily window or a
    /// Focus.
    pub fn is_quiet(&self) -> bool {
        self.focus.is_some() || self.quiet_ends(crate::panes::now_secs()).is_some()
    }

    /// When the timed part of quiet ends, from the timer and `[quiet] daily`
    /// together, or none when neither is on. A Focus has no end to count to.
    pub fn quiet_ends(&self, now: u64) -> Option<u64> {
        crate::quiet::effective_until(self.quiet_until, &self.config.quiet.daily, now)
    }

    /// The line `quiet` prints, `not quiet` included.
    pub fn quiet_status(&self, now: u64) -> String {
        crate::quiet::status(self.quiet_ends(now), self.focus.as_deref(), now)
    }

    // ── health ───────────────────────────────────────────────────────────────

    /// Remember that something failed, for the mark on the bar.
    ///
    /// `source` is the timer or the segment, and `what` is the line the mark
    /// and `doctor` show, which already names it.
    pub fn note_failure(&mut self, source: &str, what: impl Into<String>) {
        self.failures
            .insert(source.to_string(), (what.into(), Instant::now()));
    }

    /// A timer ran and it worked, so what it said when it last failed is no
    /// longer true.
    ///
    /// The health check is dropped with it, or the bar would go on showing
    /// the mark for the five seconds the check is kept.
    pub fn note_ok(&mut self, source: &str) {
        if self.failures.remove(source).is_some() {
            self.health_cache = None;
        }
    }

    /// The failures inside the health window, newest first.
    pub fn recent_failures(&self) -> Vec<String> {
        let mut recent: Vec<&(String, Instant)> = self
            .failures
            .values()
            .filter(|(_, t)| t.elapsed() < crate::segments::health::FAILURE_WINDOW)
            .collect();
        recent.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
        recent.into_iter().map(|(what, _)| what.clone()).collect()
    }

    /// Note an unclean stop of the daemon before this one.
    pub fn note_unclean(&mut self, what: String) {
        self.unclean.push((what, Instant::now()));
        self.health_cache = None;
    }

    /// The unclean stops inside the health window.
    pub fn recent_unclean(&self) -> Vec<String> {
        self.unclean
            .iter()
            .filter(|(_, t)| t.elapsed() < crate::segments::health::FAILURE_WINDOW)
            .map(|(w, _)| w.clone())
            .collect()
    }

    /// Forget every failure, because somebody has read them, and say which
    /// ones there were.
    ///
    /// Only the failures: a config edited since the daemon started, a newer
    /// binary, quiet hours and a network that is gone are how things are, and
    /// the mark would put them back on its next check.
    pub fn acknowledge_failures(&mut self) -> Vec<String> {
        let mut seen = self.recent_failures();
        seen.extend(self.recent_unclean());
        self.failures.clear();
        self.unclean.clear();
        self.health_cache = None;
        seen
    }

    /// Whether any configured segment is `health`.
    pub fn health_wanted(&self) -> bool {
        self.config
            .status
            .right
            .draws(crate::config::SegmentName::Health)
    }

    /// The last check, if it is still fresh.
    pub fn health_cached(&self) -> Option<HealthSample> {
        self.health_cache
            .as_ref()
            .filter(|(_, t)| t.elapsed() < crate::segments::health::INTERVAL)
            .map(|(s, _)| s.clone())
    }

    /// Store a check with the time it ran.
    pub fn health_store(&mut self, sample: HealthSample) {
        self.health_cache = Some((sample, Instant::now()));
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Seconds from the config to a `Duration`, falling back when the number is
/// one `Duration::from_secs_f64` would panic on.
fn secs(value: f64, fallback: Duration) -> Duration {
    if value.is_finite() && value >= 0.0 {
        Duration::from_secs_f64(value)
    } else {
        fallback
    }
}

/// The pre-config alias file, read only when `[dirs.aliases]` is empty.
///
/// `~/.yrl/lib/dir-aliases` is a path on one laptop, and it stays supported so
/// that upgrading does not silently drop somebody's labels, but the config
/// table is where these belong now.
fn load_dir_aliases() -> HashMap<PathBuf, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = PathBuf::from(home).join(".yrl/lib/dir-aliases");
    let mut map = HashMap::new();

    let Ok(contents) = std::fs::read_to_string(&path) else {
        return map;
    };

    for line in contents.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(PathBuf::from(key.trim()), value.trim().to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_directory_answer_is_given_again_until_its_database_moves() {
        let mut st = ServerState::new();
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        assert_eq!(
            st.dirs_fresh("zoxide", Some(t), true),
            None,
            "nothing kept yet"
        );
        st.dirs_store("zoxide".into(), "/a\n/b\n".into(), Some(t));
        assert_eq!(
            st.dirs_fresh("zoxide", Some(t), true).as_deref(),
            Some("/a\n/b\n")
        );
        let moved = t + Duration::from_secs(1);
        assert_eq!(
            st.dirs_fresh("zoxide", Some(moved), true),
            None,
            "a cd since"
        );
        assert_eq!(
            st.dirs_fresh("ghq", None, false),
            None,
            "kept by what was asked"
        );
    }

    use super::*;

    #[test]
    fn an_unclean_stop_is_said_until_acknowledged() {
        let mut st = ServerState::new();
        st.note_unclean("stopped uncleanly".into());
        assert_eq!(st.recent_unclean(), ["stopped uncleanly"]);
        assert_eq!(st.acknowledge_failures(), ["stopped uncleanly"]);
        assert!(st.recent_unclean().is_empty());
    }

    #[test]
    fn two_requests_for_one_repository_share_one_git_flight() {
        let mut st = ServerState::new();
        let a = st.git_flight(std::path::Path::new("/repo/a"));
        let again = st.git_flight(std::path::Path::new("/repo/a"));
        let other = st.git_flight(std::path::Path::new("/repo/b"));
        assert!(std::sync::Arc::ptr_eq(&a, &again));
        assert!(!std::sync::Arc::ptr_eq(&a, &other));
    }

    #[test]
    fn a_charging_battery_is_cached_but_for_less_time() {
        let mut st = ServerState::new();
        let charging = format!("{} 80%", crate::segments::battery::CHARGING_ICON);
        st.battery_store(charging.clone());
        assert_eq!(
            st.battery_cached(),
            Some(charging),
            "cached, not read every second"
        );
        st.battery_cache = Some(("x".into(), Instant::now() - BATTERY_CHARGING_TTL));
        assert!(st.battery_cached().is_some(), "not charging: the long TTL");
        let charging = format!("{} 80%", crate::segments::battery::CHARGING_ICON);
        st.battery_cache = Some((charging, Instant::now() - BATTERY_CHARGING_TTL));
        assert!(st.battery_cached().is_none(), "charging: the short TTL");
    }

    #[test]
    fn the_shared_pane_list_is_served_only_while_younger_than_the_asker_wants() {
        let mut st = ServerState::new();
        assert!(
            st.panes_fresh(Duration::from_secs(5)).is_none(),
            "nothing read yet"
        );
        st.panes_store(Vec::new());
        assert!(st.panes_fresh(Duration::from_secs(5)).is_some());
        assert!(
            st.panes_fresh(Duration::ZERO).is_none(),
            "an asker that wants it fresher than now reads it again"
        );
    }

    fn status(branch: &str) -> GitStatus {
        GitStatus {
            branch: branch.into(),
            remote_success: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_timer_that_works_again_takes_back_its_own_failure_and_no_other() {
        let mut st = ServerState::new();
        st.note_failure("sessions autosave", "sessions autosave: disk full");
        st.note_failure("net segment", "net segment: no counters");
        assert_eq!(st.recent_failures().len(), 2);

        st.health_store(HealthSample::default());
        st.note_ok("sessions autosave");
        assert_eq!(st.recent_failures(), ["net segment: no counters"]);
        assert!(
            st.health_cached().is_none(),
            "the bar would show the old mark for five more seconds"
        );

        // Nothing taken back, nothing thrown away.
        st.health_store(HealthSample::default());
        st.note_ok("sessions autosave");
        assert!(st.health_cached().is_some());
    }

    #[test]
    fn a_second_failure_of_one_timer_replaces_its_first() {
        let mut st = ServerState::new();
        st.note_failure("autosave", "autosave: exited Some(1)");
        st.note_failure("autosave", "autosave: exited Some(2)");
        assert_eq!(st.recent_failures(), ["autosave: exited Some(2)"]);
    }

    #[test]
    fn acknowledging_forgets_every_failure_and_says_which() {
        let mut st = ServerState::new();
        assert!(st.acknowledge_failures().is_empty());
        st.note_failure("sessions autosave", "sessions autosave: disk full");
        st.health_store(HealthSample::default());
        assert_eq!(st.acknowledge_failures(), ["sessions autosave: disk full"]);
        assert!(st.recent_failures().is_empty());
        assert!(st.health_cached().is_none());
        // One that fails after that is news again.
        st.note_failure("sessions autosave", "sessions autosave: disk full");
        assert_eq!(st.recent_failures().len(), 1);
    }

    #[test]
    fn git_cache_round_trips() {
        let mut st = ServerState::new();
        let p = PathBuf::from("/tmp/repo");
        assert!(st.git_cached(&p, DEFAULT_GST_TTL).is_none());
        st.git_store(p.clone(), status("main"), DEFAULT_GST_TTL);
        assert_eq!(
            st.git_cached(&p, DEFAULT_GST_TTL).map(|s| s.branch),
            Some("main".to_string())
        );
    }

    #[test]
    fn git_cache_respects_a_zero_ttl_flag() {
        // `--ttl 0` means "never serve from cache", including immediately after
        // the write — the flag applies from the first hit.
        let mut st = ServerState::new();
        let p = PathBuf::from("/tmp/repo");
        st.git_store(p.clone(), status("main"), Duration::ZERO);
        assert!(st.git_cached(&p, Duration::ZERO).is_none());
    }

    #[test]
    fn git_cache_is_keyed_by_path() {
        let mut st = ServerState::new();
        let a = PathBuf::from("/tmp/a");
        let b = PathBuf::from("/tmp/b");
        st.git_store(a.clone(), status("main"), DEFAULT_GST_TTL);
        st.git_store(b.clone(), status("dev"), DEFAULT_GST_TTL);
        assert_eq!(
            st.git_cached(&a, DEFAULT_GST_TTL).map(|s| s.branch),
            Some("main".to_string())
        );
        assert_eq!(
            st.git_cached(&b, DEFAULT_GST_TTL).map(|s| s.branch),
            Some("dev".to_string())
        );
    }

    #[test]
    fn repo_check_cache_round_trips_both_answers() {
        let mut st = ServerState::new();
        let yes = PathBuf::from("/tmp/repo");
        let no = PathBuf::from("/tmp/plain");
        assert_eq!(st.repo_cached(&yes), None);
        st.repo_store(yes.clone(), true);
        st.repo_store(no.clone(), false);
        assert_eq!(st.repo_cached(&yes), Some(true));
        // A negative answer is cached too, otherwise every non-repo pane keeps
        // paying the fork on every refresh.
        assert_eq!(st.repo_cached(&no), Some(false));
    }

    #[test]
    fn repo_check_ttl_is_bounded_so_git_init_is_noticed() {
        // The point of the TTL: it must not be "forever".
        assert!(REPO_CHECK_TTL <= Duration::from_secs(600));
        assert!(REPO_CHECK_TTL >= Duration::from_secs(60));
    }

    #[test]
    fn default_gst_ttl_is_five_seconds() {
        assert_eq!(DEFAULT_GST_TTL, Duration::from_secs(5));
    }

    #[test]
    fn battery_cache_round_trips() {
        let mut st = ServerState::new();
        assert!(st.battery_cached().is_none());
        st.battery_store("100%".into());
        assert_eq!(st.battery_cached(), Some("100%".to_string()));
    }

    #[test]
    fn agents_cache_round_trips_and_is_off_the_default_side() {
        let mut st = ServerState::new();
        assert!(!st.agents_wanted(), "the default side does not read panes");
        assert!(st.agents_cached().is_none());
        let sample = AgentsSample {
            total: 2,
            waiting: 1,
            busy: 1,
        };
        st.agents_store(sample);
        assert_eq!(st.agents_cached(), Some(sample));
        assert_eq!(st.agents_interval(), Duration::from_secs(2));

        st.config
            .status
            .right
            .segments
            .push(crate::config::RightSegment {
                name: crate::config::SegmentName::Agents,
                separator_before: String::new(),
                on_click: String::new(),
            });
        assert!(st.agents_wanted());
    }

    #[test]
    fn net_state_starts_empty() {
        let st = ServerState::new();
        assert!(st.net_previous.is_none());
        assert!(st.net_last_render.is_empty());
    }
}
