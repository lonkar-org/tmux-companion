//! The local daemon's links: one per host in `[[hosts]]` and `@tc-hosts`,
//! each up only while its mode says something needs it, asking `sessions`
//! every `poll_secs` while it is, and retried at 5 s doubling to 5 minutes
//! when it fails.
//!
//! Each host gets a thread of its own with a single-threaded runtime, because
//! Cap'n Proto's RPC system is not `Send` and the daemon's runtime is
//! multi-threaded. A thread that is waiting costs nothing; one per host is
//! the cost the design budgets, along with one ssh process while the link is
//! up.
//!
//! What the threads learn goes into [`Links`] on `ServerState`, under the
//! same rule as every other field there: compute first, then lock for one
//! statement.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify};

use super::{Hello, Segment, Session, cache};
use crate::{
    config::{Host, LinkMode},
    server::state::ServerState,
};

/// The first wait after a failed link.
pub const RETRY_FIRST: Duration = Duration::from_secs(5);

/// The longest wait between two tries.
pub const RETRY_MAX: Duration = Duration::from_secs(300);

/// How often a `while-attached` host looks at whether anybody is attached.
/// The answer comes from the daemon's shared pane poll, so this reads a list
/// some other loop usually already paid for.
const ATTACH_CHECK: Duration = Duration::from_secs(5);

/// How long a mirror session keeps its host's link wanted after the mirror
/// loop last saw it: a few of that loop's ticks, so a link never closes under
/// a mirror session somebody is in.
const MIRROR_HOLD: Duration = Duration::from_secs(10);

/// How often a link with an attached mirror session asks for its segments:
/// the bar's own tick.
const SEGMENT_TICK: Duration = Duration::from_secs(1);

/// How often the cache file is rewritten when nothing in it but the time
/// changed. The age a picker shows is in minutes past the first one, so a
/// minute of drift after a restart is not something anybody reads.
const CACHE_REWRITE: Duration = Duration::from_secs(60);

/// Where a host came from.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kept {
    /// `[[hosts]]` in config.toml.
    Config,
    /// `@tc-hosts` on the tmux server, until it exits.
    Server,
}

/// Where a link is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum State {
    /// `link = "off"`.
    Off,
    /// Nothing has needed it yet, or since it was closed.
    Idle,
    /// The host command is running and the hello hasn't come back.
    Connecting,
    /// It said hello.
    Up,
    /// It failed; `retry_at` is when it is tried next, unix seconds.
    Down {
        /// Why, as one line.
        reason: String,
        /// Unix seconds.
        retry_at: u64,
    },
}

/// A thread's switches: a wake for an ask, and a stop for a host that went.
#[derive(Debug, Default)]
pub struct Control {
    /// Rung by an ask, which also resets the backoff.
    pub wake: Notify,
    /// Set when the host was removed or its config changed.
    pub stop: AtomicBool,
    /// Bumped each time a connect attempt ends, either way.
    pub attempts: AtomicU64,
}

/// One host as the daemon holds it.
#[derive(Debug)]
pub struct HostLink {
    /// The host's settings.
    pub host: Host,
    /// Where it came from.
    pub kept: Kept,
    /// Where the link is.
    pub state: State,
    /// Unix seconds of the last answer, 0 for never.
    pub heard_at: u64,
    /// Unix seconds the host's daemon read the sessions.
    pub polled_at: u64,
    /// The last hello, kept after the link goes.
    pub hello: Option<Hello>,
    /// The host's build, from the hello or the cache.
    pub build: String,
    /// The last sessions answer.
    pub sessions: Vec<Session>,
    /// When something last asked for this host.
    pub wanted_at: Option<Instant>,
    /// How long the last hello took, from starting the host command to its
    /// answer, in milliseconds; what the benchmark reads.
    pub hello_ms: u64,
    /// The thread's switches.
    pub control: Arc<Control>,
    /// Unix seconds of the last connect attempt, 0 for none.
    pub tried_at: u64,
    /// When the mirror loop last saw a mirror session to this host open.
    pub mirrored_at: Option<Instant>,
    /// The grouped sessions on the host whose segments the mirror bar
    /// wants, one per mirror session with a client attached here.
    pub segment_wants: Vec<String>,
    /// The last segments heard for each grouped session, with when (unix
    /// seconds).
    pub segments: std::collections::HashMap<String, (Vec<Segment>, u64)>,
    /// Whether the link is up and watching, the host pushing, rather than
    /// polled.
    pub watching: bool,
    /// The agents waiting on the host, from its last `inbox` answer; empty
    /// whenever the link isn't up, since an old list would read as now.
    pub inbox: Vec<crate::inbox::Entry>,
    /// Every entry the last answer held, by [`entry_key`], so the next one
    /// sounds only for what is new. Kept while the link is down, so a link
    /// coming back doesn't sound for what was already waiting.
    inbox_known: std::collections::HashSet<String>,
    /// Whether an inbox answer has come from this host since the daemon
    /// started: the first one only learns what is already waiting.
    inbox_seeded: bool,
    /// The host's directories from its last `dirs` answer.
    pub dirs: Vec<String>,
    /// When they arrived.
    pub dirs_at: Option<Instant>,
    /// When a picker last asked for them; the link fetches them when this
    /// is newer than [`HostLink::dirs_at`].
    pub dirs_asked: Option<Instant>,
    /// The host saying no to `dirs`, and when.
    pub dirs_refused: Option<(String, Instant)>,
    written: Option<(Instant, Vec<Session>)>,
}

impl HostLink {
    fn new(host: Host, kept: Kept) -> Self {
        let state = if host.link == LinkMode::Off {
            State::Off
        } else {
            State::Idle
        };
        Self {
            host,
            kept,
            state,
            heard_at: 0,
            polled_at: 0,
            hello: None,
            build: String::new(),
            sessions: Vec::new(),
            wanted_at: None,
            hello_ms: 0,
            control: Arc::new(Control::default()),
            tried_at: 0,
            mirrored_at: None,
            segment_wants: Vec::new(),
            segments: std::collections::HashMap::new(),
            watching: false,
            inbox: Vec::new(),
            inbox_known: std::collections::HashSet::new(),
            inbox_seeded: false,
            dirs: Vec::new(),
            dirs_at: None,
            dirs_asked: None,
            dirs_refused: None,
            written: None,
        }
    }

    /// Whether the link should be up now, given whether any client is
    /// attached to the local server.
    fn wanted(&self, attached: bool) -> bool {
        let asked = self
            .wanted_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(self.host.idle_secs));
        let mirrored = self
            .mirrored_at
            .is_some_and(|at| at.elapsed() < MIRROR_HOLD);
        match self.host.link {
            LinkMode::Off => false,
            LinkMode::OnDemand => asked || mirrored,
            LinkMode::WhileAttached => attached || asked || mirrored,
        }
    }
}

impl HostLink {
    /// How long until whether the link is wanted could change by itself:
    /// an ask or a mirror running out, or the next look at whether anybody
    /// is attached. A watching link has nothing else waking it.
    fn recheck_in(&self) -> Duration {
        let left =
            |at: Option<Instant>, hold: Duration| at.and_then(|t| hold.checked_sub(t.elapsed()));
        let lapse = [
            left(self.wanted_at, Duration::from_secs(self.host.idle_secs)),
            left(self.mirrored_at, MIRROR_HOLD),
        ]
        .into_iter()
        .flatten()
        .max()
        .map(|d| d + Duration::from_millis(50));
        match self.host.link {
            LinkMode::WhileAttached => lapse.map_or(ATTACH_CHECK, |d| d.min(ATTACH_CHECK)),
            _ => lapse.unwrap_or(Duration::from_secs(60)),
        }
    }
}

/// What `hosts` and `doctor` are told about one host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HostView {
    /// The host's name.
    pub name: String,
    /// Its command.
    pub command: Vec<String>,
    /// Its mode.
    pub link: LinkMode,
    /// Where it came from.
    pub kept: Kept,
    /// Where the link is.
    pub state: State,
    /// Unix seconds of the last answer, 0 for never.
    pub heard_at: u64,
    /// The host's build.
    pub build: String,
    /// The host's tmux, from the last hello.
    pub tmux: String,
    /// Whether the bridge started the host's daemon.
    pub started_daemon: bool,
    /// The methods the host's bridge lacks, once a hello has said.
    pub lacks: Vec<String>,
    /// Connect attempts ended so far.
    pub attempts: u64,
    /// The last sessions answer.
    pub sessions: Vec<Session>,
    /// How long the last hello took, in milliseconds.
    #[serde(default)]
    pub hello_ms: u64,
    /// Unix seconds of the last connect attempt, 0 for none.
    #[serde(default)]
    pub tried_at: u64,
    /// The agents waiting on the host while its link is up, each with the
    /// host's name in [`crate::inbox::Entry::host`].
    #[serde(default)]
    pub inbox: Vec<crate::inbox::Entry>,
    /// What picking one of the host's sessions does.
    #[serde(default)]
    pub open: crate::config::HostOpen,
    /// Whether the link is up and the host pushing, rather than polled.
    #[serde(default)]
    pub pushing: bool,
    /// The host's `poll_secs`: how often a link it doesn't push is polled.
    #[serde(default)]
    pub poll_secs: u64,
}

/// What tells two inbox entries apart for the sound: the pane, when it
/// stopped and how. An agent that stopped again is a new entry.
pub fn entry_key(e: &crate::inbox::Entry) -> String {
    format!("{}\t{}\t{}", e.id, e.since, e.state)
}

/// The sound an inbox answer earns against what was known before: `Asked`
/// when anything new asked or went quiet, `Done` when everything new said
/// `done`, nothing when nothing is new. One sound per answer, as the local
/// inbox makes one per pass.
pub fn arrival_sound(
    known: &std::collections::HashSet<String>,
    entries: &[crate::inbox::Entry],
) -> Option<crate::config::EarconEvent> {
    let new: Vec<&crate::inbox::Entry> = entries
        .iter()
        .filter(|e| !known.contains(&entry_key(e)))
        .collect();
    if new.is_empty() {
        None
    } else if new.iter().any(|e| e.state != "done") {
        Some(crate::config::EarconEvent::Asked)
    } else {
        Some(crate::config::EarconEvent::Done)
    }
}

/// Every host, in order: config first, then the tmux server's.
#[derive(Debug, Default)]
pub struct Links {
    /// The hosts.
    pub hosts: Vec<HostLink>,
}

impl Links {
    /// One host, by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut HostLink> {
        self.hosts.iter_mut().find(|h| h.host.name == name)
    }

    /// One view per host.
    pub fn views(&self) -> Vec<HostView> {
        self.hosts
            .iter()
            .map(|h| HostView {
                name: h.host.name.clone(),
                command: h.host.command.clone(),
                link: h.host.link,
                kept: h.kept,
                state: h.state.clone(),
                heard_at: h.heard_at,
                build: h.build.clone(),
                tmux: h.hello.as_ref().map(|x| x.tmux.clone()).unwrap_or_default(),
                started_daemon: h.hello.as_ref().is_some_and(|x| x.started_daemon),
                lacks: h
                    .hello
                    .as_ref()
                    .map(|x| x.lacks().iter().map(|m| m.to_string()).collect())
                    .unwrap_or_default(),
                attempts: h.control.attempts.load(Ordering::Relaxed),
                sessions: h.sessions.clone(),
                hello_ms: h.hello_ms,
                tried_at: h.tried_at,
                inbox: h.inbox.clone(),
                open: h.host.open,
                pushing: h.watching,
                poll_secs: h.host.poll_secs,
            })
            .collect()
    }

    /// Every host's waiting agents, for the local inbox: each host's last
    /// answer while its link is up, nothing from one that is down.
    pub fn inbox(&self) -> Vec<crate::inbox::Entry> {
        self.hosts
            .iter()
            .filter(|h| h.state == State::Up)
            .flat_map(|h| h.inbox.iter().cloned())
            .collect()
    }

    /// Keep an inbox answer from `name`, each entry marked as the host's,
    /// and say which sound it earns: none for the first answer since the
    /// daemon started, which only learns what was already waiting.
    pub fn inbox_heard(
        &mut self,
        name: &str,
        mut entries: Vec<crate::inbox::Entry>,
    ) -> Option<crate::config::EarconEvent> {
        let h = self.get_mut(name)?;
        for e in &mut entries {
            e.host = name.to_string();
        }
        let sound = arrival_sound(&h.inbox_known, &entries).filter(|_| h.inbox_seeded);
        h.inbox_seeded = true;
        h.inbox_known = entries.iter().map(entry_key).collect();
        h.inbox = entries;
        sound
    }

    /// A picker wants `name`'s directories: the ones heard within `fresh`
    /// at once, or `None` after marking the ask and waking the link, which
    /// fetches them. `Err` for a host nobody has.
    pub fn ask_dirs(&mut self, name: &str, fresh: Duration) -> Result<Option<Vec<String>>, String> {
        let Some(h) = self.get_mut(name) else {
            return Err(format!("no host called {name}; `hosts` lists them"));
        };
        if h.dirs_at.is_some_and(|at| at.elapsed() < fresh) {
            return Ok(Some(h.dirs.clone()));
        }
        h.dirs_asked = Some(Instant::now());
        h.wanted_at = Some(Instant::now());
        h.control.wake.notify_one();
        Ok(None)
    }

    /// The answer to an ask for `name`'s directories made at `since` after
    /// `attempts` connect attempts, once there is one: the directories, the
    /// host's refusal, a bridge too old to serve them, or the link failing
    /// on a try made after the ask.
    pub fn dirs_answer(
        &self,
        name: &str,
        since: Instant,
        attempts: u64,
    ) -> Option<Result<Vec<String>, String>> {
        let h = self.hosts.iter().find(|h| h.host.name == name)?;
        if h.dirs_at.is_some_and(|at| at >= since) {
            return Some(Ok(h.dirs.clone()));
        }
        if let Some((why, at)) = &h.dirs_refused
            && *at >= since
        {
            return Some(Err(format!("{name}: {why}")));
        }
        if h.state == State::Up && h.hello.as_ref().is_some_and(|x| !x.serves("dirs")) {
            return Some(Err(format!(
                "{name} runs a tmux-companion whose bridge has no dirs: upgrade it there"
            )));
        }
        match &h.state {
            State::Down { reason, .. } if h.control.attempts.load(Ordering::Relaxed) > attempts => {
                Some(Err(format!("{name} can't be reached: {reason}")))
            }
            State::Off => Some(Err(format!("{name}'s link is off"))),
            _ => None,
        }
    }

    /// Connect attempts `name`'s link has ended so far.
    pub fn attempts(&self, name: &str) -> u64 {
        self.hosts
            .iter()
            .find(|h| h.host.name == name)
            .map_or(0, |h| h.control.attempts.load(Ordering::Relaxed))
    }

    /// The directories `name`'s link has to fetch: whether a picker asked
    /// after the last ones arrived.
    fn dirs_due(&self, name: &str) -> bool {
        self.hosts
            .iter()
            .find(|h| h.host.name == name)
            .is_some_and(|h| {
                h.dirs_asked
                    .is_some_and(|asked| h.dirs_at.is_none_or(|at| at < asked))
                    && h.dirs_refused
                        .as_ref()
                        .is_none_or(|(_, at)| h.dirs_asked.is_some_and(|asked| *at < asked))
            })
    }

    fn dirs_heard(&mut self, name: &str, answer: Result<Vec<String>, String>) {
        if let Some(h) = self.get_mut(name) {
            match answer {
                Ok(dirs) => {
                    h.dirs = dirs;
                    h.dirs_at = Some(Instant::now());
                }
                Err(why) => h.dirs_refused = Some((why, Instant::now())),
            }
        }
    }

    /// Tell each host which of its sessions have a mirror open here and
    /// which of those want segments for a bar: `open` names the hosts with
    /// any mirror session, `wants` the grouped sessions, by host, of the ones
    /// with a client attached. A host that gains its first mirror is woken,
    /// so its link comes up now rather than at its next look.
    pub fn set_mirrors(&mut self, open: &[String], wants: &[(String, String)]) {
        for h in &mut self.hosts {
            let is_open = open.contains(&h.host.name);
            let was = h.mirrored_at.is_some_and(|at| at.elapsed() < MIRROR_HOLD);
            if is_open {
                h.mirrored_at = Some(Instant::now());
                if !was {
                    h.control.wake.notify_one();
                }
            }
            let mine: Vec<String> = wants
                .iter()
                .filter(|(host, _)| *host == h.host.name)
                .map(|(_, g)| g.clone())
                .collect();
            // A watching host pushes a grouped session's segments once,
            // when they change, which can be before this loop has seen the
            // mirror's client; it says when one goes, so nothing is pruned.
            if !h.watching {
                h.segments.retain(|g, _| mine.contains(g));
            }
            if mine != h.segment_wants {
                let gained = h.segment_wants.is_empty() && !mine.is_empty();
                h.segment_wants = mine;
                if gained {
                    h.control.wake.notify_one();
                }
            }
        }
    }

    /// The segments last heard for one grouped session on `host`, and
    /// whether that host's link is up.
    pub fn segments_for(&self, host: &str, grouped: &str) -> (Option<(Vec<Segment>, u64)>, bool) {
        match self.hosts.iter().find(|h| h.host.name == host) {
            Some(h) => (h.segments.get(grouped).cloned(), h.state == State::Up),
            None => (None, false),
        }
    }

    fn segments_heard(&mut self, name: &str, heard: Vec<(String, Vec<Segment>)>, now: u64) {
        if let Some(h) = self.get_mut(name) {
            for (g, segs) in heard {
                if h.segment_wants.contains(&g) {
                    h.segments.insert(g, (segs, now));
                }
            }
        }
    }

    /// Segments a watching host pushed for one of its grouped sessions:
    /// kept, or forgotten when the list is empty, which is the host saying
    /// the session lost its client or went.
    fn segments_pushed(&mut self, name: &str, grouped: String, segs: Vec<Segment>, now: u64) {
        if let Some(h) = self.get_mut(name) {
            if segs.is_empty() {
                h.segments.remove(&grouped);
            } else {
                h.segments.insert(grouped, (segs, now));
            }
        }
    }

    /// The host answered a ping: it was heard from now.
    fn pong(&mut self, name: &str, now: u64) {
        if let Some(h) = self.get_mut(name)
            && h.state == State::Up
        {
            h.heard_at = now;
        }
    }

    fn set_watching(&mut self, name: &str, watching: bool) {
        if let Some(h) = self.get_mut(name) {
            h.watching = watching;
        }
    }

    /// Forget one cached session row of a host: gone from memory now, and
    /// from the cache file; the next poll brings it back if it is still
    /// there. Returns the cache to write, or `None` when nothing matched.
    pub fn forget_row(&mut self, name: &str, session: &str) -> Option<cache::Cached> {
        let h = self.get_mut(name)?;
        let before = h.sessions.len();
        h.sessions.retain(|s| s.name != session);
        if h.sessions.len() == before {
            return None;
        }
        h.written = Some((Instant::now(), h.sessions.clone()));
        Some(cache::Cached {
            heard_at: h.heard_at,
            polled_at: h.polled_at,
            build: h.build.clone(),
            sessions: h.sessions.clone(),
        })
    }

    /// Something needs `name`'s link, or every host's with `None`: mark the
    /// ask and wake the thread, which starts or retries the link at once.
    /// `false` for a name no host has.
    pub fn want(&mut self, name: Option<&str>) -> bool {
        let mut found = false;
        for h in &mut self.hosts {
            if name.is_none_or(|n| n == h.host.name) {
                found = true;
                h.wanted_at = Some(Instant::now());
                h.control.wake.notify_one();
            }
        }
        found
    }

    /// The host list replaced by `hosts`, keeping what is known about every
    /// host whose settings didn't change. Returns the hosts whose threads
    /// have to start; a host that went or changed has its thread stopped.
    pub fn replace(&mut self, hosts: Vec<(Host, Kept)>) -> Vec<(String, Arc<Control>)> {
        let mut old: Vec<HostLink> = std::mem::take(&mut self.hosts);
        let mut start = Vec::new();
        for (host, kept) in hosts {
            match old.iter().position(|h| h.host == host) {
                Some(i) => {
                    let mut kept_link = old.remove(i);
                    kept_link.kept = kept;
                    self.hosts.push(kept_link);
                }
                None => {
                    let mut fresh = HostLink::new(host, kept);
                    // A changed command or mode keeps the rows it had.
                    if let Some(i) = old.iter().position(|h| h.host.name == fresh.host.name) {
                        let was = old.remove(i);
                        was.control.stop.store(true, Ordering::Relaxed);
                        was.control.wake.notify_one();
                        fresh.heard_at = was.heard_at;
                        fresh.polled_at = was.polled_at;
                        fresh.build = was.build;
                        fresh.sessions = was.sessions;
                    }
                    if fresh.host.link != LinkMode::Off {
                        start.push((fresh.host.name.clone(), Arc::clone(&fresh.control)));
                    }
                    self.hosts.push(fresh);
                }
            }
        }
        for gone in old {
            gone.control.stop.store(true, Ordering::Relaxed);
            gone.control.wake.notify_one();
        }
        start
    }

    /// A cached answer read from disk, for a host that hasn't been heard
    /// from by this daemon yet.
    pub fn seed(&mut self, name: &str, c: cache::Cached) {
        if let Some(h) = self.get_mut(name)
            && h.heard_at == 0
        {
            h.heard_at = c.heard_at;
            h.polled_at = c.polled_at;
            h.build = c.build;
            h.sessions = c.sessions;
        }
    }

    fn set_state(&mut self, name: &str, state: State) {
        if let Some(h) = self.get_mut(name) {
            if state != State::Up {
                // The inbox is the host's only while it can be heard, and
                // so are the bar's segments.
                h.inbox.clear();
                h.segments.clear();
                h.watching = false;
            }
            h.state = state;
        }
    }

    fn hello(&mut self, name: &str, hello: Hello, now: u64, took: Duration) {
        if let Some(h) = self.get_mut(name) {
            h.hello_ms = took.as_millis() as u64;
            h.build = hello.build.clone();
            h.hello = Some(hello);
            h.heard_at = now;
            h.state = State::Up;
        }
    }

    /// Keep a sessions answer, and say whether the cache file is due a
    /// write: when the sessions changed, or a minute has passed.
    fn heard(
        &mut self,
        name: &str,
        polled_at: u64,
        sessions: Vec<Session>,
        now: u64,
    ) -> Option<cache::Cached> {
        let h = self.get_mut(name)?;
        h.heard_at = now;
        h.polled_at = polled_at;
        h.sessions = sessions;
        let due = match &h.written {
            None => true,
            Some((at, rows)) => *rows != h.sessions || at.elapsed() >= CACHE_REWRITE,
        };
        if !due {
            return None;
        }
        h.written = Some((Instant::now(), h.sessions.clone()));
        Some(cache::Cached {
            heard_at: h.heard_at,
            polled_at: h.polled_at,
            build: h.build.clone(),
            sessions: h.sessions.clone(),
        })
    }
}

/// The hosts kept on the tmux server, `@tc-hosts`, read now. Empty when
/// there is no server or no option.
pub async fn server_hosts() -> Vec<Host> {
    let out =
        crate::cli::tmux_capture(&["show-options", "-sqv", crate::hosts::SERVER_OPTION]).await;
    crate::hosts::parse_server_hosts(&out)
}

/// Every host, config first, then the tmux server's, each name once.
fn all_hosts(config: &[Host], server: Vec<Host>) -> Vec<(Host, Kept)> {
    let mut out: Vec<(Host, Kept)> = config.iter().cloned().map(|h| (h, Kept::Config)).collect();
    for h in server {
        if !out.iter().any(|(x, _)| x.name == h.name) {
            out.push((h, Kept::Server));
        }
    }
    out
}

/// Load the hosts, seed them from their cache files and start a thread for
/// each one whose mode isn't `off`. Called once when the daemon starts,
/// before it accepts anything: the config's hosts are there from the first
/// answer, and the tmux server's join them a moment later, since asking tmux
/// is a process the first answer shouldn't wait on.
pub async fn start(state: &Arc<Mutex<ServerState>>) {
    let config_hosts = state.lock().await.config.hosts.clone();
    load(state, &config_hosts, Vec::new()).await;
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let server = server_hosts().await;
        if !server.is_empty() {
            load(&state, &config_hosts, server).await;
        }
    });
}

/// Read the config and the tmux server's hosts again, for `hosts add` and
/// anything else that changed them, and start or stop threads to match.
pub async fn reload(state: &Arc<Mutex<ServerState>>) -> anyhow::Result<()> {
    let config = crate::config::load().map_err(|e| anyhow::anyhow!("{e}"))?.0;
    load(state, &config.hosts, server_hosts().await).await;
    Ok(())
}

async fn load(state: &Arc<Mutex<ServerState>>, config_hosts: &[Host], server: Vec<Host>) {
    let hosts = all_hosts(config_hosts, server);
    let names: Vec<String> = hosts.iter().map(|(h, _)| h.name.clone()).collect();
    let cached: Vec<(String, cache::Cached)> = match cache::dir() {
        Some(dir) => names
            .iter()
            .filter_map(|n| cache::read(&cache::path_in(&dir, n)).map(|c| (n.clone(), c)))
            .collect(),
        None => Vec::new(),
    };
    let start = {
        let mut st = state.lock().await;
        let start = st.links.replace(hosts);
        for (name, c) in cached {
            st.links.seed(&name, c);
        }
        start
    };
    for (name, control) in start {
        spawn(name, Arc::clone(state), control);
    }
}

/// One host's thread.
fn spawn(name: String, state: Arc<Mutex<ServerState>>, control: Arc<Control>) {
    let thread = format!("link-{name}");
    let spawned = std::thread::Builder::new().name(thread).spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("tmux-companion: no runtime for the {name} link: {e}");
                return;
            }
        };
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, host_loop(name, state, control));
    });
    if let Err(e) = spawned {
        eprintln!("tmux-companion: cannot start a link thread: {e}");
    }
}

/// The host's settings and whether its link should be up; `None` when the
/// host is gone.
async fn wanted(name: &str, state: &Arc<Mutex<ServerState>>) -> Option<(Host, bool)> {
    let mode = {
        let st = state.lock().await;
        st.links
            .hosts
            .iter()
            .find(|h| h.host.name == name)
            .map(|h| h.host.link)?
    };
    let attached = if mode == LinkMode::WhileAttached {
        super::local_sessions(state, ATTACH_CHECK)
            .await
            .iter()
            .any(|s| s.attached > 0)
    } else {
        false
    };
    let st = state.lock().await;
    let h = st.links.hosts.iter().find(|h| h.host.name == name)?;
    Some((h.host.clone(), h.wanted(attached)))
}

/// Whether `name`'s last hello listed `method`.
async fn link_serves(state: &Arc<Mutex<ServerState>>, name: &str, method: &str) -> bool {
    state
        .lock()
        .await
        .links
        .hosts
        .iter()
        .find(|h| h.host.name == name)
        .and_then(|h| h.hello.as_ref())
        .is_some_and(|x| x.serves(method))
}

async fn set_state(state: &Arc<Mutex<ServerState>>, name: &str, s: State) {
    state.lock().await.links.set_state(name, s);
}

/// Wait out a failure: the backoff, or less when something asks, which also
/// starts the count again. Returns the next backoff.
async fn back_off(control: &Control, backoff: Duration) -> Duration {
    tokio::select! {
        _ = control.wake.notified() => RETRY_FIRST,
        _ = tokio::time::sleep(backoff) => (backoff * 2).min(RETRY_MAX),
    }
}

async fn host_loop(name: String, state: Arc<Mutex<ServerState>>, control: Arc<Control>) {
    let stopped = || control.stop.load(Ordering::Relaxed);
    let mut backoff = RETRY_FIRST;
    loop {
        // Until something needs the link.
        let host = loop {
            if stopped() {
                return;
            }
            let Some((host, want)) = wanted(&name, &state).await else {
                return;
            };
            if host.link == LinkMode::Off {
                set_state(&state, &name, State::Off).await;
                return;
            }
            if want {
                break host;
            }
            let check = if host.link == LinkMode::WhileAttached {
                ATTACH_CHECK
            } else {
                Duration::from_secs(60)
            };
            tokio::select! {
                _ = control.wake.notified() => {}
                _ = tokio::time::sleep(check) => {}
            }
        };

        {
            let mut st = state.lock().await;
            st.links.set_state(&name, State::Connecting);
            if let Some(h) = st.links.get_mut(&name) {
                h.tried_at = crate::panes::now_secs();
            }
        }
        let began = Instant::now();
        let opened = super::conn::open(&host).await;
        let took = began.elapsed();
        control.attempts.fetch_add(1, Ordering::Relaxed);
        let (mut link, hello) = match opened {
            Ok(v) => v,
            Err(reason) => {
                let retry_at = crate::panes::now_secs() + backoff.as_secs();
                set_state(&state, &name, State::Down { reason, retry_at }).await;
                backoff = back_off(&control, backoff).await;
                continue;
            }
        };
        backoff = RETRY_FIRST;
        let watch = hello.serves("watch") && hello.serves("ping");
        state
            .lock()
            .await
            .links
            .hello(&name, hello, crate::panes::now_secs(), took);

        let up = if watch {
            watching(&name, &host, &mut link, &state, &control).await
        } else {
            polling(&name, &host, &mut link, &state, &control).await
        };
        match up {
            Up::Stopped => {
                link.close().await;
                return;
            }
            Up::Failed(reason) => {
                let retry_at = crate::panes::now_secs() + backoff.as_secs();
                set_state(&state, &name, State::Down { reason, retry_at }).await;
                backoff = back_off(&control, backoff).await;
            }
            Up::Unwanted => {
                link.close().await;
                set_state(&state, &name, State::Idle).await;
            }
        }
    }
}

/// How a link that was up came down.
enum Up {
    /// The host was removed or its settings changed.
    Stopped,
    /// Nothing needs it any more.
    Unwanted,
    /// It broke, said why; the link is already stopped.
    Failed(String),
}

/// Pings in a row a watching link may leave unanswered; the next tick after
/// the last of them restarts it.
pub const PINGS_MISSED: u32 = 3;

/// A sessions answer kept, the cache file written when it is due.
async fn sessions_heard(
    state: &Arc<Mutex<ServerState>>,
    name: &str,
    polled_at: u64,
    mut sessions: Vec<Session>,
) {
    super::order(&mut sessions);
    let now = crate::panes::now_secs();
    let due = state
        .lock()
        .await
        .links
        .heard(name, polled_at, sessions, now);
    if let (Some(c), Some(dir)) = (due, cache::dir())
        && let Err(e) = cache::write(&cache::path_in(&dir, name), &c)
    {
        eprintln!("tmux-companion: could not keep {name}'s sessions: {e}");
    }
}

/// An inbox answer kept, and the sound it earns played.
async fn inbox_heard(
    state: &Arc<Mutex<ServerState>>,
    name: &str,
    entries: Vec<crate::inbox::Entry>,
) {
    let (sound, earcons, quiet) = {
        let mut st = state.lock().await;
        let sound = st.links.inbox_heard(name, entries);
        (sound, st.config.earcons.clone(), st.is_quiet())
    };
    // The host's agents sound as the local ones do, and are held by this
    // machine's quiet the same way, whatever made it quiet.
    if let Some(event) = sound {
        crate::earcons::sound(&earcons, event, quiet);
    }
}

/// The object a watching host calls: one per link, holding nothing but the
/// host's name and the daemon's state. Everything it is handed is read
/// through the same cleaning and caps as an answer to a question.
struct Sink {
    name: String,
    state: Arc<Mutex<ServerState>>,
}

impl crate::link_capnp::session_sink::Server for Sink {
    async fn sessions(
        self: std::rc::Rc<Self>,
        params: crate::link_capnp::session_sink::SessionsParams,
        _: crate::link_capnp::session_sink::SessionsResults,
    ) -> Result<(), capnp::Error> {
        let p = params.get()?;
        let polled_at = p.get_polled_at();
        let sessions = super::read_sessions(p.get_sessions()?)?;
        sessions_heard(&self.state, &self.name, polled_at, sessions).await;
        Ok(())
    }

    async fn inbox(
        self: std::rc::Rc<Self>,
        params: crate::link_capnp::session_sink::InboxParams,
        _: crate::link_capnp::session_sink::InboxResults,
    ) -> Result<(), capnp::Error> {
        let entries = super::read_entries(params.get()?.get_entries()?)?;
        inbox_heard(&self.state, &self.name, entries).await;
        Ok(())
    }

    async fn segments(
        self: std::rc::Rc<Self>,
        params: crate::link_capnp::session_sink::SegmentsParams,
        _: crate::link_capnp::session_sink::SegmentsResults,
    ) -> Result<(), capnp::Error> {
        let p = params.get()?;
        let grouped = super::clean(&String::from_utf8_lossy(p.get_session()?.as_bytes()));
        let segs = super::read_segments(p.get_segments()?)?;
        self.state.lock().await.links.segments_pushed(
            &self.name,
            grouped,
            segs,
            crate::panes::now_secs(),
        );
        Ok(())
    }
}

/// Whether a ping tick finds the link dead: `answered` of `sent` pings
/// came back, and `misses` ticks in a row found the last one unanswered.
/// Returns the new count of misses.
pub fn ping_tick(sent: u64, answered: u64, misses: u32) -> u32 {
    if sent > answered { misses + 1 } else { 0 }
}

/// Up and watching: the host pushes the sessions, the inbox and the
/// segments of this machine's grouped sessions when they change, so this
/// loop only pings, fetches directories when a picker asks, and looks at
/// whether the link is still wanted. Nothing here runs on a timer but the
/// ping.
async fn watching(
    name: &str,
    host: &Host,
    link: &mut super::conn::Link,
    state: &Arc<Mutex<ServerState>>,
    control: &Control,
) -> Up {
    let sink: crate::link_capnp::session_sink::Client = capnp_rpc::new_client(Sink {
        name: name.to_string(),
        state: Arc::clone(state),
    });
    let mut watch = link.watch(sink, &super::mirror::machine());
    state.lock().await.links.set_watching(name, true);
    let dirs_served = link_serves(state, name, "dirs").await;
    let every = Duration::from_secs(host.ping_secs.max(1));
    let answered = std::rc::Rc::new(std::cell::Cell::new(0u64));
    let mut sent = 0u64;
    let mut misses = 0u32;
    let mut next_ping = tokio::time::Instant::now() + every;
    loop {
        if control.stop.load(Ordering::Relaxed) {
            return Up::Stopped;
        }
        if dirs_served && state.lock().await.links.dirs_due(name) {
            match link.dirs().await {
                Ok(answer) => state.lock().await.links.dirs_heard(name, answer),
                Err(reason) => return Up::Failed(reason),
            }
        }
        match wanted(name, state).await {
            Some((now_host, true)) if now_host == *host => {}
            _ => return Up::Unwanted,
        }
        let recheck = state
            .lock()
            .await
            .links
            .hosts
            .iter()
            .find(|h| h.host.name == name)
            .map_or(Duration::from_secs(60), HostLink::recheck_in);
        tokio::select! {
            ended = &mut watch => {
                return Up::Failed(match ended {
                    Err(e) => link.broke(e).await,
                    Ok(_) => {
                        link.shut().await;
                        format!("{name}'s bridge stopped watching")
                    }
                });
            }
            _ = control.wake.notified() => {}
            _ = tokio::time::sleep(recheck) => {}
            _ = tokio::time::sleep_until(next_ping) => {
                misses = ping_tick(sent, answered.get(), misses);
                if misses >= PINGS_MISSED {
                    link.shut().await;
                    return Up::Failed(format!(
                        "no answer to {PINGS_MISSED} pings in {}s",
                        every.as_secs() * u64::from(PINGS_MISSED)
                    ));
                }
                sent += 1;
                let (seq, answered, ping) = (sent, std::rc::Rc::clone(&answered), link.ping());
                let (state, name) = (Arc::clone(state), name.to_string());
                tokio::task::spawn_local(async move {
                    ping.await;
                    answered.set(answered.get().max(seq));
                    state.lock().await.links.pong(&name, crate::panes::now_secs());
                });
                next_ping = tokio::time::Instant::now() + every;
            }
        }
    }
}

/// Up and polling, for a host whose bridge has no `watch`: `sessions` and
/// `inbox` every poll_secs, and each wanted grouped session's segments every
/// second while a mirror session has a client.
async fn polling(
    name: &str,
    host: &Host,
    link: &mut super::conn::Link,
    state: &Arc<Mutex<ServerState>>,
    control: &Control,
) -> Up {
    let (polls, inbox_served, dirs_served, segments_served) = (
        link_serves(state, name, "sessions").await,
        link_serves(state, name, "inbox").await,
        link_serves(state, name, "dirs").await,
        link_serves(state, name, "segments").await,
    );
    let poll = Duration::from_secs(host.poll_secs.max(1));
    let mut polled: Option<Instant> = None;
    loop {
        if control.stop.load(Ordering::Relaxed) {
            return Up::Stopped;
        }
        if polls && polled.is_none_or(|t| t.elapsed() >= poll) {
            polled = Some(Instant::now());
            match link.sessions().await {
                Ok((polled_at, sessions)) => {
                    sessions_heard(state, name, polled_at, sessions).await;
                }
                Err(reason) => return Up::Failed(reason),
            }
            if inbox_served {
                match link.inbox().await {
                    Ok(Ok(entries)) => inbox_heard(state, name, entries).await,
                    // The host said no; its agents are left out until it
                    // says yes.
                    Ok(Err(_)) => {}
                    Err(reason) => return Up::Failed(reason),
                }
            }
        }
        if dirs_served && state.lock().await.links.dirs_due(name) {
            match link.dirs().await {
                Ok(answer) => state.lock().await.links.dirs_heard(name, answer),
                Err(reason) => return Up::Failed(reason),
            }
        }
        let wants = if segments_served {
            state
                .lock()
                .await
                .links
                .get_mut(name)
                .map(|h| h.segment_wants.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut heard = Vec::new();
        for g in &wants {
            match link.segments(g).await {
                Ok(Ok(segs)) => heard.push((g.clone(), segs)),
                // The host said no: no such session yet, say. The bar shows
                // nothing from it until it says yes.
                Ok(Err(_)) => heard.push((g.clone(), Vec::new())),
                Err(reason) => return Up::Failed(reason),
            }
        }
        if !heard.is_empty() {
            state
                .lock()
                .await
                .links
                .segments_heard(name, heard, crate::panes::now_secs());
        }
        match wanted(name, state).await {
            Some((now_host, true)) if now_host == *host => {}
            _ => return Up::Unwanted,
        }
        let wait = if wants.is_empty() {
            poll.saturating_sub(polled.map_or(Duration::ZERO, |t| t.elapsed()))
                .max(Duration::from_millis(100))
        } else {
            SEGMENT_TICK
        };
        tokio::select! {
            _ = control.wake.notified() => {}
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str, mode: LinkMode) -> Host {
        Host {
            link: mode,
            ..Host::new(name, vec!["ssh".into(), name.into()])
        }
    }

    #[test]
    fn each_mode_wants_the_link_when_it_should() {
        let mut on = HostLink::new(host("a", LinkMode::OnDemand), Kept::Config);
        assert!(!on.wanted(true), "on-demand: no link until asked");
        on.wanted_at = Some(Instant::now());
        assert!(on.wanted(false));
        on.host.idle_secs = 1;
        on.wanted_at = Some(Instant::now() - Duration::from_secs(2));
        assert!(!on.wanted(true), "and none after idle_secs");

        let att = HostLink::new(host("b", LinkMode::WhileAttached), Kept::Config);
        assert!(att.wanted(true));
        assert!(!att.wanted(false));

        let mut off = HostLink::new(host("c", LinkMode::Off), Kept::Config);
        off.wanted_at = Some(Instant::now());
        assert!(!off.wanted(true), "off: never");
        assert_eq!(off.state, State::Off);
    }

    #[test]
    fn replacing_the_list_keeps_what_is_known_and_stops_what_went() {
        let mut links = Links::default();
        let start = links.replace(vec![
            (host("a", LinkMode::OnDemand), Kept::Config),
            (host("off", LinkMode::Off), Kept::Config),
        ]);
        assert_eq!(
            start.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["a"],
            "no thread for an off host"
        );
        let a_control = Arc::clone(&links.hosts[0].control);
        links.hosts[0].heard_at = 42;

        let start = links.replace(vec![
            (host("a", LinkMode::OnDemand), Kept::Config),
            (host("b", LinkMode::OnDemand), Kept::Server),
        ]);
        assert_eq!(
            start.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["b"]
        );
        assert_eq!(links.hosts[0].heard_at, 42, "a kept");
        assert!(!a_control.stop.load(Ordering::Relaxed));

        let mut changed = host("a", LinkMode::OnDemand);
        changed.command = vec!["ssh".into(), "elsewhere".into()];
        let start = links.replace(vec![(changed, Kept::Config)]);
        assert_eq!(start.len(), 1, "a changed command is a new thread");
        assert!(
            a_control.stop.load(Ordering::Relaxed),
            "and the old one stops"
        );
        assert_eq!(links.hosts[0].heard_at, 42, "with the rows it had");
        assert_eq!(links.hosts.len(), 1, "b went");
    }

    #[test]
    fn an_ask_wakes_the_host_and_an_unknown_name_says_so() {
        let mut links = Links::default();
        links.replace(vec![(host("a", LinkMode::OnDemand), Kept::Config)]);
        assert!(links.want(Some("a")));
        assert!(links.hosts[0].wanted_at.is_some());
        assert!(!links.want(Some("nope")));
    }

    #[test]
    fn the_cache_is_written_on_a_change_and_not_on_every_poll() {
        let mut links = Links::default();
        links.replace(vec![(host("a", LinkMode::OnDemand), Kept::Config)]);
        let rows = vec![Session {
            name: "api".into(),
            ..Session::default()
        }];
        assert!(links.heard("a", 1, rows.clone(), 10).is_some(), "first");
        assert!(links.heard("a", 2, rows.clone(), 15).is_none(), "same rows");
        assert!(links.heard("a", 3, Vec::new(), 20).is_some(), "changed");
        assert_eq!(links.hosts[0].heard_at, 20);
    }

    #[tokio::test(start_paused = true)]
    async fn the_backoff_doubles_to_five_minutes_and_an_ask_resets_it() {
        let c = Control::default();
        let mut wait = RETRY_FIRST;
        let mut seen = Vec::new();
        for _ in 0..8 {
            wait = back_off(&c, wait).await;
            seen.push(wait.as_secs());
        }
        assert_eq!(seen, [10, 20, 40, 80, 160, 300, 300, 300]);
        c.wake.notify_one();
        assert_eq!(back_off(&c, wait).await, RETRY_FIRST);
    }

    fn waiting(id: &str, since: u64, state: &str) -> crate::inbox::Entry {
        crate::inbox::Entry {
            id: id.into(),
            at: "api:1.0".into(),
            program: "claude".into(),
            path: String::new(),
            since,
            lines: String::new(),
            state: state.into(),
            question: "go on?".into(),
            host: String::new(),
        }
    }

    #[test]
    fn a_hosts_agents_sound_once_when_new_and_never_for_the_first_answer() {
        use crate::config::EarconEvent;
        let mut links = Links::default();
        links.replace(vec![(host("vm", LinkMode::OnDemand), Kept::Config)]);
        links.set_state("vm", State::Up);
        assert_eq!(
            links.inbox_heard("vm", vec![waiting("%1", 5, "asked")]),
            None,
            "the first answer only learns what was waiting"
        );
        assert_eq!(links.inbox()[0].host, "vm", "each entry says its host");
        assert_eq!(
            links.inbox_heard("vm", vec![waiting("%1", 5, "asked")]),
            None,
            "nothing new"
        );
        assert_eq!(
            links.inbox_heard(
                "vm",
                vec![waiting("%1", 5, "asked"), waiting("%2", 9, "done")]
            ),
            Some(EarconEvent::Done)
        );
        assert_eq!(
            links.inbox_heard("vm", vec![waiting("%1", 12, "asked")]),
            Some(EarconEvent::Asked),
            "stopped again is new"
        );
        links.set_state(
            "vm",
            State::Down {
                reason: "gone".into(),
                retry_at: 0,
            },
        );
        assert!(links.inbox().is_empty(), "nothing from a host that is down");
        links.set_state("vm", State::Up);
        assert_eq!(
            links.inbox_heard("vm", vec![waiting("%1", 12, "asked")]),
            None,
            "a link coming back doesn't sound for what was already there"
        );
    }

    #[test]
    fn a_dirs_ask_is_answered_from_memory_or_waits_for_the_link() {
        let mut links = Links::default();
        links.replace(vec![(host("vm", LinkMode::OnDemand), Kept::Config)]);
        let fresh = Duration::from_secs(30);
        assert_eq!(links.ask_dirs("vm", fresh), Ok(None), "nothing yet");
        assert!(links.dirs_due("vm"));
        let since = links.hosts[0].dirs_asked.expect("asked");
        assert_eq!(links.dirs_answer("vm", since, 0), None, "still waiting");
        links.dirs_heard("vm", Ok(vec!["/srv/api".into()]));
        assert!(!links.dirs_due("vm"));
        assert_eq!(
            links.dirs_answer("vm", since, 0),
            Some(Ok(vec!["/srv/api".to_string()]))
        );
        assert_eq!(
            links.ask_dirs("vm", fresh),
            Ok(Some(vec!["/srv/api".to_string()])),
            "a fresh list at once"
        );
        assert!(links.ask_dirs("nope", fresh).is_err());

        links.hosts[0].hello = Some(Hello {
            methods: vec!["hello".into(), "sessions".into()],
            ..Hello::default()
        });
        links.hosts[0].state = State::Up;
        links.hosts[0].dirs_at = None;
        let later = Instant::now();
        assert!(
            links
                .dirs_answer("vm", later, 0)
                .is_some_and(|a| a.is_err_and(|e| e.contains("no dirs"))),
            "an older bridge says so"
        );
    }

    #[test]
    fn a_watching_link_looks_again_when_an_ask_would_run_out() {
        let mut on = HostLink::new(host("a", LinkMode::OnDemand), Kept::Config);
        assert_eq!(
            on.recheck_in(),
            Duration::from_secs(60),
            "nothing to run out"
        );
        on.host.idle_secs = 2;
        on.wanted_at = Some(Instant::now());
        let d = on.recheck_in();
        assert!(
            d > Duration::from_secs(1) && d <= Duration::from_millis(2050),
            "{d:?}"
        );
        on.mirrored_at = Some(Instant::now());
        assert!(
            on.recheck_in() > Duration::from_secs(9),
            "the mirror holds it longer"
        );
        let att = HostLink::new(host("b", LinkMode::WhileAttached), Kept::Config);
        assert_eq!(att.recheck_in(), ATTACH_CHECK);
    }

    #[test]
    fn three_pings_unanswered_in_a_row_and_the_link_is_restarted() {
        // A tick before the first ping finds nothing owed.
        assert_eq!(ping_tick(0, 0, 0), 0);
        // Each tick finds the last ping unanswered: one, two, three.
        let mut misses = 0;
        for sent in 1..=PINGS_MISSED as u64 {
            misses = ping_tick(sent, 0, misses);
        }
        assert_eq!(misses, PINGS_MISSED);
        // An answer, even a late one, starts the count again.
        assert_eq!(ping_tick(3, 3, 2), 0);
        assert_eq!(ping_tick(4, 3, 0), 1, "one owed is one miss");
    }

    #[test]
    fn pushed_segments_are_kept_until_the_host_says_they_went() {
        let mut links = Links::default();
        links.replace(vec![(host("vm", LinkMode::OnDemand), Kept::Config)]);
        links.set_state("vm", State::Up);
        links.set_watching("vm", true);
        let segs = vec![Segment {
            role: "git".into(),
            text: "main".into(),
            ..Segment::default()
        }];
        // Before the mirror loop has seen the mirror's client.
        links.segments_pushed("vm", "api~mac".into(), segs.clone(), 7);
        links.set_mirrors(&[], &[]);
        assert_eq!(
            links.segments_for("vm", "api~mac"),
            (Some((segs, 7)), true),
            "a watching host's push isn't pruned"
        );
        links.segments_pushed("vm", "api~mac".into(), Vec::new(), 8);
        assert_eq!(links.segments_for("vm", "api~mac").0, None);
        links.segments_pushed("vm", "web~mac".into(), vec![Segment::default()], 9);
        links.set_state(
            "vm",
            State::Down {
                reason: "gone".into(),
                retry_at: 0,
            },
        );
        assert_eq!(links.segments_for("vm", "web~mac"), (None, false));
        assert!(!links.hosts[0].watching);
    }

    #[test]
    fn a_seed_from_disk_never_overwrites_what_this_daemon_heard() {
        let mut links = Links::default();
        links.replace(vec![(host("a", LinkMode::OnDemand), Kept::Config)]);
        let c = cache::Cached {
            heard_at: 5,
            build: "old".into(),
            ..cache::Cached::default()
        };
        links.seed("a", c.clone());
        assert_eq!(links.hosts[0].heard_at, 5);
        links.hosts[0].heard_at = 9;
        links.seed("a", c);
        assert_eq!(links.hosts[0].heard_at, 9);
    }
}
