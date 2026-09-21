use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{
    cache::TtlMap, config::Config, segments::git::GitStatus, segments::network::NetSample,
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
    /// Previous cumulative rx/tx counters, for the bandwidth delta.
    pub net_previous: Option<NetSample>,
    /// The last bandwidth string rendered, replayed when two samples arrive too
    /// close together to divide by (see `segments::network`).
    pub net_last_render: String,
    /// Directory-to-label overrides for the window segment.
    pub dir_aliases: HashMap<PathBuf, String>,
    /// Cached battery render with the time it was computed.
    pub battery_cache: Option<(String, Instant)>,
    /// Parsed `git status`, keyed by canonicalized repository path.
    git_cache: TtlMap<PathBuf, GitStatus>,
    /// Whether a path is inside a git work tree, keyed by canonicalized path.
    repo_check: TtlMap<PathBuf, bool>,
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
        Self {
            config,
            net_previous: None,
            net_last_render: String::new(),
            dir_aliases: if config_aliases.is_empty() {
                load_dir_aliases()
            } else {
                config_aliases
            },
            battery_cache: None,
            git_cache: TtlMap::new(),
            repo_check: TtlMap::new(),
        }
    }

    // ── git status ───────────────────────────────────────────────────────────

    /// The configured `git status` freshness window.
    pub fn git_ttl(&self) -> Duration {
        secs(self.config.git.ttl_secs, DEFAULT_GST_TTL)
    }

    /// The configured is-inside-work-tree freshness window.
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

    /// The last battery render, if it is still fresh.
    pub fn battery_cached(&self) -> Option<String> {
        self.battery_cache
            .as_ref()
            .filter(|(_, t)| t.elapsed() < self.battery_ttl())
            .map(|(s, _)| s.clone())
    }

    /// Store a battery render with the time it was computed.
    pub fn battery_store(&mut self, rendered: String) {
        self.battery_cache = Some((rendered, Instant::now()));
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
    use super::*;

    fn status(branch: &str) -> GitStatus {
        GitStatus {
            branch: branch.into(),
            remote_success: true,
            ..Default::default()
        }
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
    fn net_state_starts_empty() {
        let st = ServerState::new();
        assert!(st.net_previous.is_none());
        assert!(st.net_last_render.is_empty());
    }
}
