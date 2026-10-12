//! The link to another host's daemon: Cap'n Proto RPC over the stdin and
//! stdout of one host command, `ssh vm` running `tmux-companion bridge`.
//!
//! `bridge` is the host's half, `conn` opens one link and asks it things,
//! `manager` keeps one link per host up when something needs it, and `cache`
//! keeps each host's last answer on disk. The schema is `schema/link.capnp`
//! and docs/dev/design-remote-sessions.md is the design.
//!
//! Everything the host sends is untrusted: [`clean`] runs over every string
//! that arrives and [`MAX_SESSIONS`] caps every list, on the local side, before
//! anything stores or prints it.

pub mod bridge;
pub mod cache;
pub mod conn;
pub mod manager;
pub mod mirror;

use serde::{Deserialize, Serialize};

use crate::link_capnp;

/// The schema's number, said in the hello. Bumped only for a change an
/// older end would misread, which adding a field or a method is not.
pub const PROTOCOL: u16 = 1;

/// Every method this build's bridge answers, in the schema's order.
///
/// `dirs`, then `watch` and `ping`, came after the first bridges shipped.
/// None needed a new protocol number: an older bridge's hello doesn't list
/// them, so the local side never asks that host for them (it polls instead
/// of watching), and `hosts` says the host's bridge lacks them.
pub const METHODS: &[&str] = &[
    "hello", "sessions", "segments", "inbox", "dirs", "watch", "ping",
];

/// The remote command the link appends to a host's argv. One argument, a
/// shell line, because ssh joins its arguments into one for the remote shell.
pub const BRIDGE_LINE: &str = "tmux-companion bridge";

/// The most sessions taken from one answer; the rest are dropped.
pub const MAX_SESSIONS: usize = 500;

/// The most directories taken from one `dirs` answer: a source that knows
/// more lists the rest after anybody would scroll to them.
pub const MAX_DIRS: usize = 2000;

/// The most bytes kept of any one string from a host.
pub const MAX_TEXT: usize = 4096;

/// The most bytes of one message the link reads, in words: a full answer of
/// [`MAX_SESSIONS`] sessions with every string at [`MAX_TEXT`] is about 16 MB,
/// so this lets an honest answer through and stops a host that sends more.
const TRAVERSAL_WORDS: usize = 4 * 1024 * 1024;

/// How the link reads a message: a size cap and the default nesting.
pub fn reader_options() -> capnp::message::ReaderOptions {
    let mut o = capnp::message::ReaderOptions::new();
    o.traversal_limit_in_words(Some(TRAVERSAL_WORDS));
    o
}

/// One session on a host, as its daemon's pane poll last read it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Session {
    /// `#{session_name}`.
    pub name: String,
    /// `#{session_id}`, the `$N`.
    pub id: String,
    /// `#{session_windows}`.
    pub windows: u32,
    /// Clients attached to it on the host.
    pub attached: u32,
    /// `#{session_last_attached}`, unix seconds, 0 for never.
    pub last_attached: u64,
    /// `#{session_activity}`, unix seconds.
    pub activity: u64,
    /// `@tc_ses_used`, the switcher's stamp, 0 for none.
    pub used: u64,
    /// `#{session_path}`.
    pub path: String,
}

/// One segment of a session's bar, as structured fields rather than markup.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Segment {
    /// `git`, `agents` or `health`.
    pub role: String,
    /// What it says.
    pub text: String,
    /// A colour name, empty for the bar's own.
    pub fg: String,
    /// A colour name, empty for the bar's own.
    pub bg: String,
    /// An `icons.rs` constant's name, empty for none.
    pub glyph: String,
}

/// The sessions answer: when the host's daemon read them, and the list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionsAnswer {
    /// Unix seconds.
    pub polled_at: u64,
    /// Every session but the mirror ones.
    pub sessions: Vec<Session>,
}

/// The segments answer for one session.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct SegmentsAnswer {
    /// Unix seconds.
    pub polled_at: u64,
    /// Git, agents and health, each only when it has something to say.
    pub segments: Vec<Segment>,
}

/// What a bridge's `watch` is told each time its daemon wakes it: the
/// generation to wait past next, and everything it pushes, for it to send on
/// what changed.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchAnswer {
    /// The daemon's `watch_gen` when this was read.
    pub generation: u64,
    /// The sessions, as `sessions` answers them.
    pub sessions: SessionsAnswer,
    /// This machine's own waiting agents, longest wait first.
    pub inbox: Vec<crate::inbox::Entry>,
    /// Each grouped session with a client whose `@tc-mirror` is the machine
    /// asked about, with its segments.
    pub segments: Vec<(String, Vec<Segment>)>,
}

/// Untrusted text made safe to keep and print: control characters gone
/// (a newline included, since every string here is one line), and cut to
/// [`MAX_TEXT`] bytes on a character boundary.
pub fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(MAX_TEXT));
    for c in s.chars() {
        if c.is_control() {
            continue;
        }
        if out.len() + c.len_utf8() > MAX_TEXT {
            break;
        }
        out.push(c);
    }
    out
}

/// Text off the wire, as lossy UTF-8 and then [`clean`].
fn text(t: capnp::Result<capnp::text::Reader<'_>>) -> capnp::Result<String> {
    Ok(clean(&String::from_utf8_lossy(t?.as_bytes())))
}

/// One session off the wire, cleaned.
pub fn read_session(r: link_capnp::session::Reader<'_>) -> capnp::Result<Session> {
    Ok(Session {
        name: text(r.get_name())?,
        id: text(r.get_id())?,
        windows: r.get_windows(),
        attached: r.get_attached(),
        last_attached: r.get_last_attached(),
        activity: r.get_activity(),
        used: r.get_used(),
        path: text(r.get_path())?,
    })
}

/// A session list off the wire: at most [`MAX_SESSIONS`], each cleaned.
pub fn read_sessions(
    list: capnp::struct_list::Reader<'_, link_capnp::session::Owned>,
) -> capnp::Result<Vec<Session>> {
    list.iter()
        .take(MAX_SESSIONS)
        .map(read_session)
        .collect::<capnp::Result<Vec<_>>>()
}

/// Write sessions into a list builder sized for them.
pub fn write_sessions(
    mut list: capnp::struct_list::Builder<'_, link_capnp::session::Owned>,
    sessions: &[Session],
) {
    for (i, s) in sessions.iter().enumerate() {
        let mut b = list.reborrow().get(i as u32);
        b.set_name(&s.name);
        b.set_id(&s.id);
        b.set_windows(s.windows);
        b.set_attached(s.attached);
        b.set_last_attached(s.last_attached);
        b.set_activity(s.activity);
        b.set_used(s.used);
        b.set_path(&s.path);
    }
}

/// A segment list off the wire, each string cleaned, at most eight.
pub fn read_segments(
    list: capnp::struct_list::Reader<'_, link_capnp::segment::Owned>,
) -> capnp::Result<Vec<Segment>> {
    list.iter()
        .take(8)
        .map(|r| {
            Ok(Segment {
                role: text(r.get_role())?,
                text: text(r.get_text())?,
                fg: text(r.get_fg())?,
                bg: text(r.get_bg())?,
                glyph: text(r.get_glyph())?,
            })
        })
        .collect()
}

/// An inbox off the wire, each string cleaned, at most [`MAX_SESSIONS`].
pub fn read_entries(
    list: capnp::struct_list::Reader<'_, link_capnp::entry::Owned>,
) -> capnp::Result<Vec<crate::inbox::Entry>> {
    list.iter()
        .take(MAX_SESSIONS)
        .map(|r| {
            Ok(crate::inbox::Entry {
                id: text(r.get_pane())?,
                at: text(r.get_at())?,
                program: text(r.get_program())?,
                path: text(r.get_path())?,
                since: r.get_since(),
                lines: String::new(),
                state: text(r.get_state())?,
                question: text(r.get_question())?,
                host: String::new(),
            })
        })
        .collect()
}

/// A directory list off the wire: at most [`MAX_DIRS`], each cleaned, the
/// empty ones dropped.
pub fn read_dirs(list: capnp::text_list::Reader<'_>) -> capnp::Result<Vec<String>> {
    let mut out = Vec::new();
    for t in list.iter().take(MAX_DIRS) {
        let d = clean(&String::from_utf8_lossy(t?.as_bytes()));
        if !d.trim().is_empty() {
            out.push(d);
        }
    }
    Ok(out)
}

/// The first field of every line [`SESSION_FORMAT`] produces, so the session
/// lines can ride in the same tmux call as the pane list and be told apart.
const SESSION_MARK: &str = "tc-session";

/// The `list-sessions -F` the daemon's pane poll chains after `list-panes`.
/// The last three fields are the mirror options: `@tc-mirror` on a grouped
/// session another machine attached to, `@tc-host` and `@tc-remote` on a
/// mirror session this machine opened.
pub const SESSION_FORMAT: &str = "tc-session\t#{session_name}\t#{session_id}\t#{session_windows}\t#{session_attached}\t#{session_last_attached}\t#{session_activity}\t#{@tc_ses_used}\t#{session_path}\t#{@tc-mirror}\t#{@tc-host}\t#{@tc-remote}";

/// How many fields a [`SESSION_FORMAT`] line has.
const SESSION_FIELDS: usize = 12;

/// A `list-sessions -f` filter that leaves out every grouped session another
/// machine's mirror attached to: what each of this machine's own session
/// lists passes, so the switcher, the project picker, `brief`, `sessions
/// idle`, snapshots and `continue` never offer the other machine's view
/// back. `-f` is in tmux 3.4, the floor.
pub const NOT_MIRRORED: &str = "#{==:#{@tc-mirror},}";

/// The session option a grouped session carries: the name of the machine
/// whose mirror session attached to it.
pub const MIRROR_OPTION: &str = "@tc-mirror";

/// The session option a mirror session carries: its host's name.
pub const HOST_OPTION: &str = "@tc-host";

/// The session option a mirror session carries: the session on the host.
pub const REMOTE_OPTION: &str = "@tc-remote";

/// One mirror session on this machine: the local session that stands for a
/// session on a host.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mirror {
    /// The local session's name, `api@vm`.
    pub session: String,
    /// The host's name.
    pub host: String,
    /// The session on the host.
    pub remote: String,
    /// Clients attached to the mirror session here.
    pub attached: u32,
}

/// What one listing in [`SESSION_FORMAT`] says: the sessions a bridge
/// answers with, the mirror sessions this machine opened, and the grouped
/// sessions another machine's mirrors attached to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    /// Every session but the grouped ones.
    pub sessions: Vec<Session>,
    /// Mirror sessions, by `@tc-host`.
    pub mirrors: Vec<Mirror>,
    /// The names of the grouped sessions.
    pub grouped: Vec<String>,
    /// The grouped sessions with a client attached, each with the machine
    /// its `@tc-mirror` names.
    pub shown: Vec<(String, String)>,
}

/// Read a listing that holds [`SESSION_FORMAT`] lines among others.
pub fn parse_listing(text: &str) -> Listing {
    let num = |s: &str| s.trim().parse::<u64>().unwrap_or(0);
    let mut out = Listing::default();
    for l in text.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != SESSION_FIELDS || f[0] != SESSION_MARK {
            continue;
        }
        if !f[9].trim().is_empty() {
            out.grouped.push(f[1].to_string());
            if num(f[4]) > 0 {
                out.shown.push((f[1].to_string(), f[9].trim().to_string()));
            }
            continue;
        }
        let session = Session {
            name: f[1].to_string(),
            id: f[2].trim().to_string(),
            windows: num(f[3]) as u32,
            attached: num(f[4]) as u32,
            last_attached: num(f[5]),
            activity: num(f[6]),
            used: num(f[7]),
            path: f[8].to_string(),
        };
        if !f[10].trim().is_empty() {
            out.mirrors.push(Mirror {
                session: session.name.clone(),
                host: f[10].trim().to_string(),
                remote: f[11].to_string(),
                attached: session.attached,
            });
        }
        out.sessions.push(session);
    }
    out
}

/// The session lines in a listing, leaving out every other line and every
/// session carrying `@tc-mirror`: that is another machine's view, a grouped
/// session nobody here made, and listing it would offer the view back.
pub fn parse_sessions(text: &str) -> Vec<Session> {
    parse_listing(text).sessions
}

/// Whether two session lists differ in anything a person reads off them.
/// `activity` moves whenever a pane prints, so it counts only when it moved
/// into another minute: ages are shown in minutes at their finest, and a
/// busy pane would otherwise make every listing news.
pub fn sessions_differ(a: &[Session], b: &[Session]) -> bool {
    a.len() != b.len()
        || a.iter().zip(b).any(|(x, y)| {
            Session {
                activity: x.activity / 60,
                ..x.clone()
            } != Session {
                activity: y.activity / 60,
                ..y.clone()
            }
        })
}

/// Sessions in the host's own recency: the switcher's stamp first, then when
/// each was last attached, then by name so two never-used ones keep a stable
/// order. The same rule the local session list follows.
pub fn order(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        b.used
            .cmp(&a.used)
            .then(b.last_attached.cmp(&a.last_attached))
            .then(a.name.cmp(&b.name))
    });
}

/// A word quoted for a POSIX shell: as it is when it is plain, in single
/// quotes otherwise, with each `'` closed, escaped and reopened.
///
/// Plain is narrow on purpose. The remote shell may be zsh, which expands a
/// leading `=` and treats `%` and `~` specially, so none of those count.
///
/// ssh hands its arguments to the remote shell joined into one line, so a
/// session called `x'; touch pwned; '` is a command unless every name that
/// goes into that line passes through here.
pub fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@+,".contains(c));
    if plain {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// This machine's sessions, mirror ones left out, from the daemon's shared
/// pane poll when it is younger than `max_age` and read now otherwise. The
/// session list rides in the poll's own tmux call, so the link costs this
/// machine no process the poll wasn't already starting.
pub async fn local_sessions(
    state: &std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
    max_age: std::time::Duration,
) -> Vec<Session> {
    if let Some((sessions, _)) = state.lock().await.sessions_fresh(max_age) {
        return sessions;
    }
    let (panes, listing) = crate::panes::list_with_sessions().await;
    let sessions = listing.sessions.clone();
    let mut st = state.lock().await;
    st.panes_store(panes);
    st.sessions_store(listing);
    sessions
}

/// A git status in words, for the `git` segment a bridge sends: the branch
/// and each count that isn't zero, `main, 2 ahead, 3 changed`. None outside a
/// repository, where the status has neither a branch nor a commit and a
/// segment would say `detached at` and nothing after it.
pub fn git_words(s: &crate::segments::git::GitStatus) -> Option<String> {
    if s.branch.is_empty() && s.commit.is_empty() {
        return None;
    }
    let mut parts = vec![if s.branch.is_empty() {
        format!("detached at {}", s.commit)
    } else {
        s.branch.clone()
    }];
    let staged =
        s.staged.added + s.staged.deleted + s.staged.modified + s.staged.renamed + s.staged.copied;
    let changed = s.unstaged.added
        + s.unstaged.deleted
        + s.unstaged.modified
        + s.unstaged.renamed
        + s.unstaged.copied;
    for (n, word) in [
        (s.ahead, "ahead"),
        (s.behind, "behind"),
        (staged, "staged"),
        (changed, "changed"),
        (s.unmerged, "conflicted"),
        (s.untracked, "untracked"),
        (s.stashed, "stashed"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {word}"));
        }
    }
    Some(parts.join(", "))
}

/// The agent count in words, for the `agents` segment a bridge sends.
pub fn agents_words(total: usize, waiting: usize, busy: usize) -> String {
    let mut s = if total == 1 {
        "1 agent".to_string()
    } else {
        format!("{total} agents")
    };
    if waiting > 0 {
        s.push_str(&format!(", {waiting} waiting"));
    }
    if busy > 0 {
        s.push_str(&format!(", {busy} busy"));
    }
    s
}

/// The hello as the local side reads it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Hello {
    /// The host's build id.
    pub build: String,
    /// The schema number it speaks.
    pub protocol: u16,
    /// Every method it answers.
    pub methods: Vec<String>,
    /// Whether the bridge had to start the host's daemon.
    pub started_daemon: bool,
    /// The host's `tmux -V`.
    pub tmux: String,
}

impl Hello {
    /// Whether the host answers `method`. The local side asks nothing the
    /// hello didn't list, whichever build is newer.
    pub fn serves(&self, method: &str) -> bool {
        self.methods.iter().any(|m| m == method)
    }

    /// The methods this build would use that the host's bridge lacks, in
    /// the schema's order.
    pub fn lacks(&self) -> Vec<&'static str> {
        METHODS
            .iter()
            .copied()
            .filter(|m| !self.serves(m))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_words_says_nothing_outside_a_repository() {
        use crate::segments::git::GitStatus;
        assert_eq!(git_words(&GitStatus::default()), None);
        let detached = GitStatus {
            commit: "abc1234".into(),
            ..GitStatus::default()
        };
        assert_eq!(git_words(&detached).as_deref(), Some("detached at abc1234"));
        let branch = GitStatus {
            branch: "main".into(),
            ahead: 2,
            ..GitStatus::default()
        };
        assert_eq!(git_words(&branch).as_deref(), Some("main, 2 ahead"));
    }

    #[test]
    fn clean_strips_control_characters_and_caps_the_length() {
        assert_eq!(clean("a\x1b[31mb\nc\td\x07"), "a[31mbcd");
        let long = "é".repeat(MAX_TEXT);
        let cut = clean(&long);
        assert!(cut.len() <= MAX_TEXT);
        assert!(cut.chars().all(|c| c == 'é'), "cut on a char boundary");
        assert_eq!(clean("#(touch pwned)"), "#(touch pwned)", "text, kept");
    }

    #[test]
    fn quoting_survives_every_quote_and_shell_character() {
        assert_eq!(quote("api"), "api");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("x'; touch pwned; '"), r"'x'\''; touch pwned; '\'''");
        for nasty in [
            "a b", "$HOME", "`id`", "a\nb", "\"q\"", "'", "a;b|c&d", "$(id)", "*", "~", "=api",
            "%3",
        ] {
            let q = quote(nasty);
            let out = std::process::Command::new("sh")
                .args(["-c", &format!("printf %s {q}")])
                .output()
                .expect("sh runs");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                nasty,
                "{nasty:?} quoted as {q}"
            );
        }
    }

    #[test]
    fn session_lines_ride_with_the_pane_list_and_mirrors_are_left_out() {
        let listing = "work\t1\tzsh\t0\t%1\tzsh\t/tmp\th\t1\t1\t1\t5\t0\th\t0\t$1\t\t42\n\
                       tc-session\tapi\t$1\t3\t1\t100\t200\t300\t/src/api\t\t\t\n\
                       tc-session\tapi~mac\t$2\t3\t1\t100\t200\t0\t/src/api\tmac\t\t\n\
                       tc-session\tx'; touch pwned; '\t$3\t1\t0\t\t5\t\t/\t\t\t\n\
                       tc-session\tweb@vm\t$4\t1\t2\t0\t5\t\t/\t\tvm\tweb\n";
        let s = parse_sessions(listing);
        let names: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["api", "x'; touch pwned; '", "web@vm"]);
        let all = parse_listing(listing);
        assert_eq!(all.grouped, ["api~mac"]);
        assert_eq!(
            all.shown,
            [("api~mac".to_string(), "mac".to_string())],
            "a grouped session with a client is one a watch pushes"
        );
        assert_eq!(
            all.mirrors,
            [Mirror {
                session: "web@vm".into(),
                host: "vm".into(),
                remote: "web".into(),
                attached: 2,
            }]
        );
        assert_eq!(s[0].windows, 3);
        assert_eq!(s[0].used, 300);
        assert_eq!(s[1].last_attached, 0, "never attached reads as 0");
        assert_eq!(
            crate::panes::parse(listing).len(),
            1,
            "the pane parser skips the session lines"
        );
    }

    #[test]
    fn a_listing_is_news_only_when_a_person_would_read_it_differently() {
        let s = |name: &str, activity: u64| Session {
            name: name.into(),
            activity,
            ..Session::default()
        };
        let a = vec![s("api", 600), s("web", 0)];
        assert!(!sessions_differ(&a, &a.clone()));
        assert!(
            !sessions_differ(&a, &[s("api", 659), s("web", 0)]),
            "a pane printing within the minute"
        );
        assert!(sessions_differ(&a, &[s("api", 660), s("web", 0)]));
        assert!(sessions_differ(&a, &[s("api", 600)]), "one went");
        assert!(sessions_differ(&a, &[s("api", 600), s("www", 0)]));
    }

    #[test]
    fn rows_follow_the_hosts_own_recency() {
        let s = |name: &str, used: u64, last: u64| Session {
            name: name.into(),
            used,
            last_attached: last,
            ..Session::default()
        };
        let mut v = vec![
            s("old", 0, 10),
            s("b", 0, 0),
            s("stamped", 50, 1),
            s("recent", 0, 90),
            s("a", 0, 0),
        ];
        order(&mut v);
        let names: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["stamped", "recent", "old", "a", "b"]);
    }

    #[test]
    fn the_hello_gates_every_method() {
        let h = Hello {
            methods: vec!["hello".into(), "sessions".into()],
            ..Hello::default()
        };
        assert!(h.serves("sessions"));
        assert!(!h.serves("segments"));
        assert!(!h.serves("dirs"), "nothing the hello didn't list");
        assert_eq!(h.lacks(), ["segments", "inbox", "dirs", "watch", "ping"]);
        let all = Hello {
            methods: METHODS.iter().map(|m| m.to_string()).collect(),
            ..Hello::default()
        };
        assert!(all.lacks().is_empty());
    }

    #[test]
    fn sessions_round_trip_through_the_schema_and_arrive_capped() {
        let many: Vec<Session> = (0..MAX_SESSIONS + 20)
            .map(|i| Session {
                name: format!("s{i}\x1b"),
                windows: 2,
                ..Session::default()
            })
            .collect();
        let mut msg = capnp::message::Builder::new_default();
        {
            let root = msg.init_root::<link_capnp::host_cache::Builder>();
            write_sessions(root.init_sessions(many.len() as u32), &many);
        }
        let bytes = capnp::serialize::write_message_to_words(&msg);
        let reader =
            capnp::serialize::read_message(&mut bytes.as_slice(), reader_options()).expect("reads");
        let root = reader
            .get_root::<link_capnp::host_cache::Reader>()
            .expect("root");
        let back = read_sessions(root.get_sessions().expect("list")).expect("sessions");
        assert_eq!(back.len(), MAX_SESSIONS);
        assert_eq!(back[0].name, "s0", "the escape is gone");
        assert_eq!(back[0].windows, 2);
    }
}
