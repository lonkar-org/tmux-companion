//! other machines' sessions in the session switcher, and the mirror sessions picking one opens
//!
//! `switch sessions` lists each host after the local sessions: one group line
//! per host in config order, `vm, 4 sessions, last heard 15m` or `live`, then
//! the host's sessions as `api@vm` in the host's own recency. The rows come
//! from the local daemon's memory or its cache on disk with the first frame;
//! the one socket question that fetches them also asks the daemon to bring
//! each on-demand link up, which it does after answering. Nothing here waits
//! on the network before a frame.
//!
//! Picking a session opens a mirror session `api@vm`: one local window whose
//! pane runs `tmux-companion host attach vm api`, the shim, which runs the
//! host command with `-t` and a grouped attach on the host. The prefix goes to
//! the host (`prefix None` on the mirror session), and a clean detach there
//! closes the mirror session and puts the client back where it came from.
//!
//! `switch sessions --print` is the source of truth the popup is a view of:
//! the same lines in the same order, with no glyph and no padding.

use crate::{
    config::Host,
    link::{
        Session,
        manager::{HostView, State},
        mirror,
    },
    proto::{HostsArgs, Request},
};

/// The status `host attach` asks the host to exit with when the session it
/// was to attach to isn't there: one the remote shell, tmux and ssh don't use
/// for anything of their own.
pub const GONE: i32 = 75;

/// What a remote row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A host's group line.
    Group(String),
    /// One of a host's sessions.
    Session {
        /// The host.
        host: String,
        /// The session on the host.
        name: String,
        /// False while the host's link is down: Enter retries instead.
        reachable: bool,
    },
}

impl Target {
    /// The host the row belongs to.
    pub fn host(&self) -> &str {
        match self {
            Target::Group(h) | Target::Session { host: h, .. } => h,
        }
    }

    /// The row's identity for `switch sessions --at`, which reopens the
    /// picker on it after a question asked on the message line.
    pub fn id(&self) -> String {
        match self {
            Target::Group(h) => format!("host:{h}"),
            Target::Session { host, name, .. } => format!("remote:{host}:{name}"),
        }
    }
}

/// One remote row: what it stands for, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// What picking it does.
    pub target: Target,
    /// The line `--print` gives it, and the words the matcher searches.
    pub line: String,
    /// The preview: why a host can't be reached, or where a session is.
    pub preview: String,
}

/// `1 session`, `4 sessions`.
fn count(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// Whether a host's rows are reachable now: anything but a link that failed.
fn reachable(v: &HostView) -> bool {
    !matches!(v.state, State::Down { .. })
}

/// Why a host can't be reached, and when that was last tried, as preview
/// lines; empty for a host that can.
pub fn reason_lines(v: &HostView, now: u64) -> String {
    let State::Down { reason, retry_at } = &v.state else {
        return String::new();
    };
    let mut out = format!("{} can't be reached\n{reason}\n", v.name);
    if v.tried_at > 0 {
        out.push_str(&format!(
            "tried {} ago\n",
            crate::sessions::idle::age(now.saturating_sub(v.tried_at))
        ));
    }
    let wait = retry_at.saturating_sub(now);
    if wait > 0 {
        out.push_str(&format!(
            "tried again in {}\n",
            crate::sessions::idle::age(wait)
        ));
    }
    out.push_str("enter tries now\n");
    out
}

/// A host's group line: `vm, 4 sessions, last heard 15m`, `vm, 4 sessions,
/// live` while the link is up, `..., unreachable` while it is down, and with
/// `print` the reason after that, `unreachable: Connection timed out`.
pub fn group_line(v: &HostView, now: u64, print: bool) -> String {
    let mut parts = vec![v.name.clone()];
    if v.heard_at == 0 && v.sessions.is_empty() {
        parts.push("never heard from".to_string());
    } else {
        parts.push(count(v.sessions.len(), "session"));
        if v.state == State::Up {
            parts.push("live".to_string());
        } else if v.heard_at > 0 {
            parts.push(format!(
                "last heard {}",
                crate::sessions::idle::age(now.saturating_sub(v.heard_at))
            ));
        }
    }
    match &v.state {
        State::Down { reason, .. } if print => parts.push(format!("unreachable: {reason}")),
        State::Down { .. } => parts.push("unreachable".to_string()),
        State::Connecting => parts.push("connecting".to_string()),
        State::Off => parts.push("link off".to_string()),
        State::Idle | State::Up => {}
    }
    parts.join(", ")
}

/// The words after a remote session's name: `idle 3d` from a day up, the
/// way local rows say it, `attached on vm` when somebody there is in it, and
/// `unreachable` while its host is.
pub fn session_words(s: &Session, host: &str, up: bool, now: u64) -> Vec<String> {
    let mut words = Vec::new();
    if let Some(idle) = crate::sessions::idle::idle_for(u64::from(s.attached), s.activity, now)
        .filter(|secs| *secs >= crate::sessions::idle::DAY)
    {
        words.push(format!("idle {}", crate::sessions::idle::age(idle)));
    }
    if s.attached > 0 {
        words.push(format!("attached on {host}"));
    }
    if !up {
        words.push("unreachable".to_string());
    }
    words
}

/// Every host's rows, in config order, each group line before its sessions.
/// A session whose mirror session is open here is left out of its group: it
/// is listed once, in the local part, where recency puts it. The group
/// line's count still counts it.
pub fn rows(views: &[HostView], open: &[(String, String)], now: u64) -> Vec<Row> {
    let mut out = Vec::new();
    for v in views {
        let up = reachable(v);
        let why = reason_lines(v, now);
        out.push(Row {
            target: Target::Group(v.name.clone()),
            line: group_line(v, now, false),
            preview: if why.is_empty() {
                format!(
                    "{}\n{}\n",
                    group_line(v, now, true),
                    crate::hosts::line(v, now)
                )
            } else {
                why.clone()
            },
        });
        for s in &v.sessions {
            if open.iter().any(|(h, r)| *h == v.name && *r == s.name) {
                continue;
            }
            let mut line = format!("{}@{}", s.name, v.name);
            for w in session_words(s, &v.name, up, now) {
                line.push_str(", ");
                line.push_str(&w);
            }
            let preview = if up {
                let mut p = format!(
                    "{} on {}\n{}\n",
                    s.name,
                    v.name,
                    count(s.windows as usize, "window")
                );
                if !s.path.is_empty() {
                    p.push_str(&s.path);
                    p.push('\n');
                }
                p
            } else {
                why.clone()
            };
            out.push(Row {
                target: Target::Session {
                    host: v.name.clone(),
                    name: s.name.clone(),
                    reachable: up,
                },
                line,
                preview,
            });
        }
    }
    out
}

/// The lines `switch sessions --print` gives after the local sessions: each
/// group line with the reason a host can't be reached after `unreachable:`,
/// then its sessions, the same order as the picker.
pub fn print_lines(views: &[HostView], open: &[(String, String)], now: u64) -> Vec<String> {
    rows(views, open, now)
        .into_iter()
        .map(|r| match &r.target {
            Target::Group(h) => views
                .iter()
                .find(|v| v.name == *h)
                .map_or(r.line, |v| group_line(v, now, true)),
            Target::Session { .. } => r.line,
        })
        .collect()
}

/// What `host` does.
#[derive(clap::Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum HostAction {
    /// Attach this terminal to SESSION on HOST through the host's command,
    /// as a grouped session of its own there: what a mirror session's pane
    /// runs. When the attach ends other than by a detach it says why and
    /// waits for r to reconnect or q to close
    Attach {
        /// The host, by its name in `hosts`
        host: String,
        /// The session on the host
        session: String,
        /// Make the session on the host first when it has none by that name
        #[arg(long)]
        create: bool,
        /// Run `tmux-companion project --detached DIR` on the host first and
        /// attach to the session it names
        #[arg(long, conflicts_with = "create")]
        dir: Option<String>,
        /// After attaching, go to this window of the session on the host
        #[arg(long)]
        window: Option<u32>,
        /// After attaching, go to this pane on the host, by its `%N` id
        #[arg(long)]
        pane: Option<String>,
        /// Say `HOST: press r to connect` and wait for r before connecting,
        /// which is how a resurrect reopens a mirror session
        #[arg(long)]
        wait: bool,
    },
}

/// What the shim does besides the plain attach: everything `host attach`
/// takes after the host and the session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shim {
    /// Make the session on the host when it has none by that name.
    pub create: bool,
    /// Start the session with `project --detached DIR` on the host first.
    pub dir: Option<String>,
    /// The window on the host to go to once attached.
    pub window: Option<u32>,
    /// The pane on the host to go to once attached, `%N`.
    pub pane: Option<String>,
    /// Wait for `r` before the first connect.
    pub wait: bool,
}

/// Whether a pane id is `%` and digits, the only shape one is: a host's
/// inbox is untrusted, and its pane id goes into a shell line on that host.
pub fn pane_id_ok(id: &str) -> bool {
    id.strip_prefix('%')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// `host`.
pub async fn run(action: HostAction) -> anyhow::Result<()> {
    match action {
        HostAction::Attach {
            host,
            session,
            create,
            dir,
            window,
            pane,
            wait,
        } => {
            if let Some(p) = &pane
                && !pane_id_ok(p)
            {
                anyhow::bail!("--pane takes a pane id, %N; {p} is not one");
            }
            let shim = Shim {
                create,
                dir,
                window,
                pane,
                wait,
            };
            attach(&host, &session, &shim).await
        }
    }
}

/// The remote rows as picker items: the group line with the ssh glyph and
/// the host's name in the host tone, each session as `api` with `@vm` in a
/// column of the host tone so the words read `api@vm`, and while a host is
/// unreachable its rows in the `offline` tone with the word on each. Group
/// lines and unreachable rows are not steps: Enter on one goes nowhere.
pub fn items(rows: &[Row], views: &[HostView], now: u64) -> Vec<crate::picker::Item> {
    use crate::picker::{Cell, Item, Tone};
    rows.iter()
        .map(|r| {
            let up = match &r.target {
                Target::Session { reachable, .. } => *reachable,
                Target::Group(h) => views.iter().find(|v| v.name == *h).is_none_or(reachable),
            };
            match &r.target {
                Target::Group(h) => {
                    let v = views.iter().find(|v| v.name == *h);
                    let rest: Vec<String> = v
                        .map(|v| group_line(v, now, false))
                        .unwrap_or_default()
                        .split(", ")
                        .skip(1)
                        .map(str::to_string)
                        .collect();
                    let tone = if up { Tone::Dim } else { Tone::Offline };
                    let mut cells = vec![Cell::new(h.clone(), Tone::Host)];
                    cells.push(Cell::new(rest.join(", "), tone));
                    Item::with_preview(r.line.clone(), r.preview.clone())
                        .in_cells(cells)
                        .with_icon(crate::tmux::icons::SSH, Tone::Host)
                        .not_a_step()
                }
                Target::Session { host, name, .. } => {
                    let (name_tone, word_tone) = if up {
                        (Tone::Strong, Tone::Dim)
                    } else {
                        (Tone::Offline, Tone::Offline)
                    };
                    let rest = r
                        .line
                        .strip_prefix(&format!("{name}@{host}"))
                        .unwrap_or("")
                        .trim_start_matches(", ")
                        .to_string();
                    let mut cells = vec![
                        Cell::new(name.clone(), name_tone),
                        Cell::new(
                            format!("@{host}"),
                            if up { Tone::Host } else { Tone::Offline },
                        ),
                    ];
                    if !rest.is_empty() {
                        cells.push(Cell::new(rest, word_tone));
                    }
                    let item = Item::with_preview(r.line.clone(), r.preview.clone())
                        .in_cells(cells)
                        .with_icon(crate::tmux::icons::SESSION, Tone::Dim);
                    if up { item } else { item.not_a_step() }
                }
            }
        })
        .collect()
}

/// The footer's words for alt-enter on a remote row, from the hosts' own
/// `open`: what it does when every host agrees, `the other way` when they
/// don't, each after the three spaces the footer's keys are set apart by.
pub fn alt_enter_hint(views: &[HostView]) -> &'static str {
    use crate::config::HostOpen;
    let mirrors = views.iter().filter(|v| v.open == HostOpen::Mirror).count();
    if mirrors == views.len() {
        "   alt-enter hand over"
    } else if mirrors == 0 {
        "   alt-enter mirror"
    } else {
        "   alt-enter the other way"
    }
}

/// The hosts as the daemon holds them, for a picker: asked of a daemon that
/// is already running, never one started for it, with an ask that brings
/// each on-demand link up after the answer. Without a daemon, the config's
/// hosts and their cache files on disk, still with no network.
pub fn views_for_picker() -> Vec<HostView> {
    let req = Request::build(
        "__hosts",
        &HostsArgs {
            connect: Some(String::new()),
            ..HostsArgs::default()
        },
    );
    if let Some(out) = crate::client::ask_running(&req, std::time::Duration::from_millis(500))
        && let Ok(views) = serde_json::from_str::<Vec<HostView>>(&out)
    {
        return views;
    }
    let config = crate::cli::config_or_default();
    let dir = crate::link::cache::dir();
    config
        .hosts
        .iter()
        .map(|h| {
            let c = dir
                .as_ref()
                .and_then(|d| crate::link::cache::read(&crate::link::cache::path_in(d, &h.name)))
                .unwrap_or_default();
            HostView {
                name: h.name.clone(),
                command: h.command.clone(),
                link: h.link,
                kept: crate::link::manager::Kept::Config,
                state: if h.link == crate::config::LinkMode::Off {
                    State::Off
                } else {
                    State::Idle
                },
                heard_at: c.heard_at,
                build: c.build,
                tmux: String::new(),
                started_daemon: false,
                lacks: Vec::new(),
                attempts: 0,
                sessions: c.sessions,
                hello_ms: 0,
                tried_at: 0,
                inbox: Vec::new(),
                open: h.open,
                pushing: false,
                poll_secs: h.poll_secs,
            }
        })
        .collect()
}

/// A host's directories, from its own configured source through its link,
/// or why there are none. Asked of the daemon, which answers from the last
/// half minute's list or fetches one, waiting at most a connect and an
/// answer.
pub async fn host_dirs(host: &str) -> Result<Vec<String>, String> {
    let resp = crate::client::send(Request::build(
        "__host_dirs",
        &crate::proto::HostDirsArgs {
            host: host.to_string(),
        },
    ))
    .await
    .map_err(|e| format!("{e:#}"))?;
    if let Some(e) = resp.error {
        return Err(e);
    }
    serde_json::from_str(&resp.output).map_err(|e| format!("{host}'s directories: {e}"))
}

/// A host's directories as project rows: `api@vm` in the words, the path as
/// the host has it, and the session the host would make for it. A directory
/// one of the host's sessions already stands for is left out, the way the
/// local list leaves out one an open session has.
pub fn dir_rows(host: &str, paths: &[String], views: &[HostView]) -> Vec<crate::project::Row> {
    let taken: Vec<&str> = views
        .iter()
        .filter(|v| v.name == host)
        .flat_map(|v| v.sessions.iter().map(|s| s.path.as_str()))
        .collect();
    let mut seen = std::collections::HashSet::new();
    paths
        .iter()
        .filter(|p| !taken.contains(&p.as_str()) && seen.insert(p.as_str()))
        .map(|p| {
            let base = p
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(p)
                .to_string();
            crate::project::Row {
                kind: crate::project::Kind::Directory,
                label: format!("{base}@{host}"),
                path: p.clone(),
                colour: None,
                idle: None,
                mirror: Some((host.to_string(), crate::project::session_name(p))),
            }
        })
        .collect()
}

/// A host by name, from the config first and then the tmux server's
/// `@tc-hosts`.
pub async fn find_host(name: &str) -> Option<Host> {
    let config = crate::cli::config_or_default();
    if let Some(h) = config.hosts.iter().find(|h| h.name == name) {
        return Some(h.clone());
    }
    crate::link::manager::server_hosts()
        .await
        .into_iter()
        .find(|h| h.name == name)
}

/// `name@host` split at its last `@`, when what follows is a host's name.
pub fn split_at_host<'a>(typed: &'a str, hosts: &[String]) -> Option<(&'a str, &'a str)> {
    let (name, host) = typed.trim().rsplit_once('@')?;
    (!name.is_empty() && hosts.iter().any(|h| h == host)).then_some((name, host))
}

/// Ask the daemon to start or retry a host's link now, and say so on the
/// message line: `vm: connecting`. What Enter on a group line or on an
/// unreachable row does.
pub async fn retry(host: &str) {
    let _ = crate::client::send(Request::build(
        "__hosts",
        &HostsArgs {
            connect: Some(host.to_string()),
            ..HostsArgs::default()
        },
    ))
    .await;
    say(&format!("{host}: connecting")).await;
}

/// One line on tmux's message line, read as text.
pub async fn say(text: &str) {
    let _ = crate::tmux::command()
        .args(["display-message", &crate::picker::literal_for_tmux(text)])
        .output()
        .await;
}

/// The shell line the mirror pane runs: this binary's `host attach`.
pub fn shim_line(exe: &str, host: &str, remote: &str, shim: &Shim) -> String {
    use crate::link::quote;
    let mut line = format!(
        "{} host attach {} {}",
        quote(exe),
        quote(host),
        quote(remote)
    );
    if shim.create {
        line.push_str(" --create");
    }
    if let Some(dir) = &shim.dir {
        line.push_str(&format!(" --dir {}", quote(dir)));
    }
    if let Some(w) = shim.window {
        line.push_str(&format!(" --window {w}"));
    }
    if let Some(p) = shim.pane.as_deref().filter(|p| pane_id_ok(p)) {
        line.push_str(&format!(" --pane {p}"));
    }
    if shim.wait {
        line.push_str(" --wait");
    }
    line
}

/// The command a snapshot records for a mirror session's pane, and the one a
/// shipped restore row claims: the shim by its name on PATH, the host and the
/// session, nothing about how it was opened.
pub fn recorded_shim(host: &str, remote: &str) -> String {
    format!(
        "tmux-companion host attach {} {}",
        crate::link::quote(host),
        crate::link::quote(remote)
    )
}

/// A restore row's command for the shim with this binary in place of the
/// name it was recorded by: a pane's shell may not have the binary on PATH,
/// and the mirror session's own pane always runs it by its path.
pub fn shim_for_restore(command: &str, exe: &str) -> String {
    match command.strip_prefix("tmux-companion ") {
        Some(rest) => format!("{} {rest}", crate::link::quote(exe)),
        None => command.to_string(),
    }
}

/// The tmux commands that make the mirror session `api@vm`: its one window
/// running the shim, then the options that make it one: `@tc-host` and
/// `@tc-remote` say what it stands for, `prefix None` hands the prefix to
/// the host, and `detach-on-destroy previous` puts the client back where it
/// came from when the attach ends.
pub fn mirror_commands(session: &str, host: &str, remote: &str, shim: &str) -> Vec<Vec<String>> {
    let target = format!("={session}:");
    let set = |opt: &str, value: &str| -> Vec<String> {
        ["set-option", "-t", &target, opt, value]
            .map(String::from)
            .to_vec()
    };
    vec![
        ["new-session", "-d", "-s", session, "-n", remote, shim]
            .map(String::from)
            .to_vec(),
        set(crate::link::HOST_OPTION, host),
        set(crate::link::REMOTE_OPTION, remote),
        set("prefix", "None"),
        set("detach-on-destroy", "previous"),
    ]
}

/// `a ; b ; c` as one tmux argv.
fn chained(cmds: Vec<Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    for c in cmds {
        if !out.is_empty() {
            out.push(";".to_string());
        }
        out.extend(c);
    }
    out
}

/// Open `remote` on `host` as a mirror session and go to it: switch to the
/// mirror session when it is open, make it otherwise, then switch (attach,
/// outside tmux) and say `api on vm` on the message line. `create` asks the
/// host to make the session first when it has none by that name; `detach`
/// makes it and goes nowhere.
///
/// A host whose `open` is `handover` hands this terminal over instead,
/// unless `detach` asks for nothing to be gone to.
pub async fn open(host: &str, remote: &str, create: bool, detach: bool) -> anyhow::Result<()> {
    let handover = !detach
        && find_host(host)
            .await
            .is_some_and(|h| h.open == crate::config::HostOpen::Handover);
    open_as(host, remote, create, detach, handover).await
}

/// [`open`] the other way from the host's `open`: what alt-enter on a
/// remote row in `M-s` does, a handover once for a host that mirrors, a
/// mirror once for one that hands over.
pub async fn open_other(host: &str, remote: &str) -> anyhow::Result<()> {
    let mirrors = find_host(host)
        .await
        .is_none_or(|h| h.open == crate::config::HostOpen::Mirror);
    open_as(host, remote, false, false, mirrors).await
}

async fn open_as(
    host: &str,
    remote: &str,
    create: bool,
    detach: bool,
    handover: bool,
) -> anyhow::Result<()> {
    if handover {
        return hand_over(host, remote, create).await;
    }
    let shim = Shim {
        create,
        ..Shim::default()
    };
    open_with(host, remote, &shim, detach).await
}

/// The remote command a handover runs on the host: this terminal attached
/// to the session itself, with the host's own bar and keys. `create` makes
/// it first when the host has none by that name.
pub fn handover_remote_line(remote: &str, create: bool) -> String {
    use crate::link::quote;
    if create {
        format!("exec tmux new-session -A -s {}", quote(remote))
    } else {
        format!(
            "exec tmux attach-session -t {}",
            quote(&format!("={remote}"))
        )
    }
}

/// The shell line `detach-client -E` runs in place of this terminal's
/// client: one plain line saying where it is going, the host command with
/// `-t` and the attach, and then `exec tmux attach` back to `left` on this
/// server, by its socket so a server started with `-L` or `-S` is the one
/// come back to. A detach on the host comes straight back; anything else
/// says so in one line and waits for Enter first, so the reason isn't wiped
/// before it is read. Every word is quoted for the shell: the session names
/// come from the host and from this server.
pub fn handover_line(host: &Host, remote: &str, create: bool, socket: &str, left: &str) -> String {
    use crate::link::quote;
    let ssh: Vec<String> = attach_argv(host, &handover_remote_line(remote, create))
        .iter()
        .map(|w| quote(w))
        .collect();
    format!(
        "printf '%s\\n' {}; {} || {{ printf '%s\\n' {}; read -r line; }}; exec tmux -S {} attach-session -t {}",
        quote(&format!("connecting to {} for {remote}", host.name)),
        ssh.join(" "),
        quote(&format!(
            "the connection to {} ended; press Enter to go back to {left}",
            host.name
        )),
        quote(socket),
        quote(&format!("={left}")),
    )
}

/// Hand this terminal to `remote` on `host`: tmux replaces this client with
/// the host command, so the host's tmux owns the whole terminal, its bar and
/// its keys, and a detach there comes back to the session this client was
/// in. Said on the message line first, since the screen is about to become
/// somebody else's. Outside tmux there is no client to replace, and the
/// attach runs in this terminal.
pub async fn hand_over(host_name: &str, remote: &str, create: bool) -> anyhow::Result<()> {
    let Some(host) = find_host(host_name).await else {
        anyhow::bail!("no host called {host_name}; `tmux-companion hosts` lists them");
    };
    if std::env::var_os("TMUX").is_none() {
        let argv = attach_argv(&host, &handover_remote_line(remote, create));
        println!("connecting to {host_name} for {remote}");
        let status = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .status()?;
        if !status.success() {
            anyhow::bail!(
                "the connection to {host_name} ended: exit status {}",
                status.code().unwrap_or(-1)
            );
        }
        return Ok(());
    }
    let here = crate::tmux::command()
        .args(["display-message", "-p", "#{socket_path}\t#{client_session}"])
        .output()
        .await?;
    let here = String::from_utf8_lossy(&here.stdout).trim_end().to_string();
    let Some((socket, left)) = here
        .split_once('\t')
        .filter(|(s, l)| !s.is_empty() && !l.is_empty())
    else {
        anyhow::bail!("no client to hand over: tmux didn't say which session this is");
    };
    let line = handover_line(&host, remote, create, socket, left);
    let said = format!("{remote} on {host_name}, handed over: detach there to come back to {left}");
    let out = crate::tmux::command()
        .args([
            "display-message",
            &crate::picker::literal_for_tmux(&said),
            ";",
            "detach-client",
            "-E",
            &line,
        ])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "could not hand over to {host_name}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Open a directory on `host` as a project there, through a mirror session:
/// the shim runs `project --detached DIR` on the host before the attach. The
/// mirror session is named for the directory the way the host's own project
/// command names its session.
pub async fn open_project(host: &str, dir: &str) -> anyhow::Result<()> {
    let remote = crate::project::session_name(dir.trim_end_matches('/'));
    let shim = Shim {
        dir: Some(dir.to_string()),
        ..Shim::default()
    };
    open_with(host, &remote, &shim, false).await
}

/// Go to an agent's pane on a host, from an inbox row: `at` is where the
/// host's inbox says it is, `session:window.pane`, and `pane` its `%N`. A
/// mirror session for that session already open has its attach respawned
/// with the pane chained after it; otherwise one opens with it. The link
/// stays read-only: moving to the pane is the attach, in the open.
pub async fn open_at_pane(host: &str, at: &str, pane: &str) -> anyhow::Result<()> {
    if !pane_id_ok(pane) {
        anyhow::bail!("{host} named a pane that isn't one: {pane}");
    }
    let (remote, rest) = at
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("{host} named no session for {pane}"))?;
    let shim = Shim {
        window: rest.split('.').next().and_then(|w| w.parse().ok()),
        pane: Some(pane.to_string()),
        ..Shim::default()
    };
    let session = mirror::session_name(remote, host);
    let open_already = crate::tmux::command()
        .args(["has-session", "-t", &format!("={session}")])
        .output()
        .await
        .is_ok_and(|o| o.status.success());
    if !open_already {
        return open_with(host, remote, &shim, false).await;
    }
    let exe = std::env::current_exe()?.display().to_string();
    let line = shim_line(&exe, host, remote, &shim);
    let mut cmds = vec![
        ["respawn-pane", "-k", "-t", &format!("={session}:"), &line]
            .map(String::from)
            .to_vec(),
    ];
    let inside = std::env::var_os("TMUX").is_some();
    if inside {
        cmds.push(go_commands(&session, remote, host));
    }
    let out = crate::tmux::command().args(chained(cmds)).output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "could not reopen {session}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    if inside {
        Ok(())
    } else {
        crate::cli::focus_session(&session).await
    }
}

/// `switch-client` to a mirror session and `api on vm` on the message line.
fn go_commands(session: &str, remote: &str, host: &str) -> Vec<String> {
    vec![
        "switch-client".into(),
        "-t".into(),
        format!("={session}"),
        ";".into(),
        "display-message".into(),
        crate::picker::literal_for_tmux(&format!("{remote} on {host}")),
    ]
}

/// [`open`] with everything the shim can do besides attach.
pub async fn open_with(host: &str, remote: &str, shim: &Shim, detach: bool) -> anyhow::Result<()> {
    let session = mirror::session_name(remote, host);
    let exe = std::env::current_exe()?.display().to_string();
    let line = shim_line(&exe, host, remote, shim);
    let inside = std::env::var_os("TMUX").is_some() && !detach;
    let go = go_commands(&session, remote, host);
    // One tmux call for the usual case, a mirror session not open yet: make
    // it, then switch and say so. tmux stops a sequence at its first error,
    // so a mirror session already open stops it at `new-session`, and the
    // switch goes alone.
    let mut cmds = mirror_commands(&session, host, remote, &line);
    if inside {
        cmds.push(go.clone());
    }
    let out = crate::tmux::command().args(chained(cmds)).output().await?;
    let already = String::from_utf8_lossy(&out.stderr).contains("duplicate session");
    if !out.status.success() && !already {
        anyhow::bail!(
            "could not open {session}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    if detach {
        println!("{session} is open");
        return Ok(());
    }
    if inside {
        if already {
            crate::tmux::command().args(&go).output().await?;
        }
        Ok(())
    } else {
        crate::cli::focus_session(&session).await
    }
}

/// The remote command the shim runs through the host command, one shell
/// line: make sure the session is there (or make it, with `create`, or start
/// it as a project with `dir`), then a grouped session for this machine's
/// mirror attached to it, with `@tc-mirror` naming this machine, its bar off
/// and gone when this machine detaches, then the window and pane asked for.
/// Every name goes through [`crate::link::quote`], since ssh joins its
/// arguments into one line for the remote shell; `machine` is already made
/// of letters, digits, `-` and `_`, and a pane is `%` and digits or left out.
pub fn remote_line(remote: &str, grouped: &str, machine: &str, shim: &Shim) -> String {
    use crate::link::quote;
    let (exact, grouped, first) = match &shim.dir {
        // The host's project command names the session, and prints the name
        // it used: the attach follows that rather than a guess made here.
        Some(dir) => (
            "\"=$s\"".to_string(),
            format!("\"$s~{machine}\""),
            format!(
                "s=$(tmux-companion project --detached {}) || exit 1; ",
                quote(dir)
            ),
        ),
        None => {
            let exact = quote(&format!("={remote}"));
            let first = if shim.create {
                format!(
                    "tmux has-session -t {exact} 2>/dev/null || tmux new-session -d -s {}; ",
                    quote(remote)
                )
            } else {
                format!("tmux has-session -t {exact} 2>/dev/null || exit {GONE}; ")
            };
            (exact, quote(grouped), first)
        }
    };
    let mut line = format!(
        "{first}exec tmux new-session -A -t {exact} -s {grouped} \\; set-option {} {} \\; set-option status off \\; set-option destroy-unattached on",
        crate::link::MIRROR_OPTION,
        quote(machine),
    );
    if let Some(w) = shim.window {
        line.push_str(&format!(" \\; select-window -t :{w}"));
    }
    if let Some(p) = shim.pane.as_deref().filter(|p| pane_id_ok(p)) {
        line.push_str(&format!(" \\; select-pane -t {p}"));
    }
    line
}

/// The argv the shim runs: the host command, `-t`, and the remote line.
pub fn attach_argv(host: &Host, line: &str) -> Vec<String> {
    let mut v = host.command.clone();
    v.push("-t".to_string());
    v.push(line.to_string());
    v
}

/// How an attach ended, as the one line the shim shows; `None` for a clean
/// detach, which closes the mirror session.
pub fn ending(host: &str, remote: &str, status: Option<i32>, tail: &[String]) -> Option<String> {
    let last = tail
        .iter()
        .rev()
        .map(|l| crate::link::clean(l.trim()))
        .find(|l| !l.is_empty());
    match status {
        Some(0) => None,
        Some(GONE) => Some(format!("{remote} no longer exists on {host}")),
        Some(code) => Some(format!(
            "the connection to {host} ended: {}",
            last.unwrap_or_else(|| format!("exit status {code}"))
        )),
        None => Some(format!(
            "the connection to {host} ended: {}",
            last.unwrap_or_else(|| "stopped by a signal".to_string())
        )),
    }
}

/// `host attach HOST SESSION`: the mirror pane's program. Says one line,
/// runs the attach in this terminal, and when it ends other than by a clean
/// detach says how in one line and waits for `r` to reconnect or `q` to
/// close. With `--wait` it waits for `r` before the first connect too, which
/// is how a resurrect reopens a dozen mirror sessions without a dozen ssh
/// handshakes at once. Nothing is timed.
pub async fn attach(host_name: &str, remote: &str, shim: &Shim) -> anyhow::Result<()> {
    use std::io::Write;
    let Some(host) = find_host(host_name).await else {
        println!("no host called {host_name}; `tmux-companion hosts` lists them");
        return wait_for_close(false).map(|_| ());
    };
    if shim.wait {
        print!("\x1b[H\x1b[2J");
        println!("{host_name}: press r to connect");
        println!("q close");
        let _ = std::io::stdout().flush();
        if wait_for_close(true)? {
            return Ok(());
        }
    }
    let machine = mirror::machine();
    let grouped = mirror::grouped_name(remote, &machine);
    let line = remote_line(remote, &grouped, &machine, shim);
    let argv = attach_argv(&host, &line);
    loop {
        print!("\x1b[H\x1b[2J");
        println!("connecting to {host_name} for {remote}");
        let _ = std::io::stdout().flush();
        let (status, tail) = run_attach(&argv);
        print!("\x1b[H\x1b[2J");
        match ending(host_name, remote, status, &tail) {
            None => return Ok(()),
            Some(said) => println!("{said}"),
        }
        println!("r reconnect   q close");
        let _ = std::io::stdout().flush();
        if !wait_for_close(true)? {
            continue;
        }
        return Ok(());
    }
}

/// Run the attach with this terminal, keeping the last lines its command
/// writes to stderr for the reason, and passing them on to the terminal as
/// they come.
fn run_attach(argv: &[String]) -> (Option<i32>, Vec<String>) {
    use std::io::BufRead;
    let child = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (Some(127), vec![format!("cannot run {}: {e}", argv[0])]),
    };
    let tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let reader = child.stderr.take().map(|err| {
        let tail = std::sync::Arc::clone(&tail);
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                eprintln!("{line}");
                let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                t.push(line);
                if t.len() > 8 {
                    t.remove(0);
                }
            }
        })
    });
    let status = child.wait().ok().and_then(|s| s.code());
    if let Some(r) = reader {
        let _ = r.join();
    }
    let tail = tail.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (status, tail)
}

/// Wait for `q` (close: true) or, when `reconnect` is offered, `r` (false).
/// Every other key is ignored; ctrl-c is `q`.
fn wait_for_close(reconnect: bool) -> anyhow::Result<bool> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return Ok(true);
    }
    ratatui::crossterm::terminal::enable_raw_mode()?;
    let answer = loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') => break true,
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break true,
                KeyCode::Char('r') if reconnect => break false,
                _ => {}
            },
            Ok(_) => {}
            Err(_) => break true,
        }
    };
    let _ = ratatui::crossterm::terminal::disable_raw_mode();
    Ok(answer)
}

/// The mirror sessions open on this tmux server, as (host, session there),
/// from the local rows the switcher already read.
pub fn open_mirrors(local: &[crate::project::Row]) -> Vec<(String, String)> {
    local.iter().filter_map(|r| r.mirror.clone()).collect()
}

/// Forget one cached row of a host. The next poll brings it back if the
/// session is still there.
pub async fn forget_row(host: &str, session: &str) {
    let _ = crate::client::send(Request::build(
        "__hosts",
        &HostsArgs {
            forget_host: Some(host.to_string()),
            forget_session: Some(session.to_string()),
            ..HostsArgs::default()
        },
    ))
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::LinkMode, link::manager::Kept};

    fn view(name: &str, state: State, sessions: &[(&str, u32)]) -> HostView {
        HostView {
            name: name.into(),
            command: vec!["ssh".into(), name.into()],
            link: LinkMode::OnDemand,
            kept: Kept::Config,
            state,
            heard_at: 1_000_000 - 900,
            build: String::new(),
            tmux: String::new(),
            started_daemon: false,
            lacks: Vec::new(),
            attempts: 0,
            sessions: sessions
                .iter()
                .map(|(n, attached)| Session {
                    name: (*n).into(),
                    windows: 2,
                    attached: *attached,
                    activity: 1_000_000 - 5 * 86_400,
                    ..Session::default()
                })
                .collect(),
            hello_ms: 0,
            tried_at: 1_000_000 - 120,
            inbox: Vec::new(),
            open: crate::config::HostOpen::Mirror,
            pushing: false,
            poll_secs: 5,
        }
    }

    const NOW: u64 = 1_000_000;

    fn down() -> State {
        State::Down {
            reason: "Connection timed out".into(),
            retry_at: NOW + 40,
        }
    }

    #[test]
    fn a_group_line_says_where_the_host_is_in_words() {
        let v = view("vm", State::Idle, &[("api", 0), ("web", 1)]);
        assert_eq!(group_line(&v, NOW, false), "vm, 2 sessions, last heard 15m");
        let up = view("vm", State::Up, &[("api", 0)]);
        assert_eq!(group_line(&up, NOW, false), "vm, 1 session, live");
        let gone = view("vm", down(), &[("api", 0)]);
        assert_eq!(
            group_line(&gone, NOW, false),
            "vm, 1 session, last heard 15m, unreachable"
        );
        assert_eq!(
            group_line(&gone, NOW, true),
            "vm, 1 session, last heard 15m, unreachable: Connection timed out"
        );
        let mut never = view("vm", State::Idle, &[]);
        never.heard_at = 0;
        assert_eq!(group_line(&never, NOW, false), "vm, never heard from");
    }

    #[test]
    fn rows_come_host_by_host_and_an_open_mirror_is_listed_once() {
        let views = vec![
            view("vm", State::Up, &[("api", 1), ("web", 0), ("db", 0)]),
            view("box", down(), &[("x", 0)]),
        ];
        let open = vec![("vm".to_string(), "web".to_string())];
        let r = rows(&views, &open, NOW);
        let lines: Vec<&str> = r.iter().map(|r| r.line.as_str()).collect();
        assert_eq!(
            lines,
            [
                "vm, 3 sessions, live",
                "api@vm, attached on vm",
                "db@vm, idle 5d",
                "box, 1 session, last heard 15m, unreachable",
                "x@box, idle 5d, unreachable",
            ]
        );
        assert!(
            r[4].preview.contains("Connection timed out"),
            "{}",
            r[4].preview
        );
        assert!(r[4].preview.contains("tried 2m ago"), "{}", r[4].preview);
        assert_eq!(
            r[4].target,
            Target::Session {
                host: "box".into(),
                name: "x".into(),
                reachable: false
            }
        );
        let it = items(&r, &views, NOW);
        assert!(it[0].not_a_step, "a group line is not a step");
        assert!(!it[1].not_a_step);
        assert!(it[4].not_a_step, "nor is an unreachable row");
        assert_eq!(
            it[1].label, "api@vm, attached on vm",
            "the words say api@vm"
        );
        assert_eq!(it[1].columns[..2], ["api", "@vm"]);
        assert_eq!(it[1].tones[1], crate::picker::Tone::Host);
        assert_eq!(it[4].tones[0], crate::picker::Tone::Offline);
    }

    #[test]
    fn a_handover_quotes_every_name_and_comes_back_to_the_session_left() {
        use std::io::Write;
        let nasty = "x'; touch pwned; '";
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = |name: &str, body: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, body).expect("fake");
            std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .expect("chmod");
            p.display().to_string()
        };
        fake(
            "tmux",
            "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done; echo\n",
        );
        // ssh as far as the line goes: the host, `-t`, then the remote
        // command run by the remote shell.
        let ssh = fake(
            "ssh",
            "#!/bin/sh\nshift\n[ \"$1\" = -t ] && shift\nexec sh -c \"$1\"\n",
        );
        let dropped = fake(
            "dropped",
            "#!/bin/sh\necho 'Connection reset' >&2\nexit 255\n",
        );
        let host = |cmd: &str| Host::new("vm", vec![cmd.to_string(), "vm".into()]);
        let run = |shell: &str, line: &str| {
            let mut child = std::process::Command::new(shell)
                .args(["-c", line])
                .current_dir(dir.path())
                .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("shell runs");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(b"\n")
                .expect("enter");
            let out = child.wait_with_output().expect("shell ends");
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        // tmux runs the line with its default-shell, which may be zsh.
        let shells: Vec<&str> = ["sh", "/bin/zsh", "/bin/bash"]
            .into_iter()
            .filter(|s| *s == "sh" || std::path::Path::new(s).exists())
            .collect();
        for shell in shells {
            let line = handover_line(&host(&ssh), nasty, false, "/tmp/a b/default", "here's");
            let said = run(shell, &line);
            let lines: Vec<&str> = said.lines().collect();
            assert_eq!(
                lines,
                [
                    "connecting to vm for x'; touch pwned; '",
                    "[attach-session][-t][=x'; touch pwned; ']",
                    "[-S][/tmp/a b/default][attach-session][-t][=here's]",
                ],
                "{shell}: {line}"
            );
            assert!(
                !dir.path().join("pwned").exists(),
                "{shell}: a name ran something"
            );

            let line = handover_line(&host(&dropped), "api", false, "/tmp/s", "here");
            let said = run(shell, &line);
            assert_eq!(
                said.lines().collect::<Vec<_>>(),
                [
                    "connecting to vm for api",
                    "the connection to vm ended; press Enter to go back to here",
                    "[-S][/tmp/s][attach-session][-t][=here]",
                ],
                "{shell}: a drop says so and waits for Enter"
            );
        }
        assert_eq!(
            handover_remote_line("api", true),
            "exec tmux new-session -A -s api"
        );
    }

    #[test]
    fn the_footer_names_what_alt_enter_does_for_these_hosts() {
        use crate::config::HostOpen;
        let mut a = view("a", State::Idle, &[]);
        let mut b = view("b", State::Idle, &[]);
        assert_eq!(
            alt_enter_hint(&[a.clone(), b.clone()]),
            "   alt-enter hand over"
        );
        a.open = HostOpen::Handover;
        assert_eq!(
            alt_enter_hint(&[a.clone(), b.clone()]),
            "   alt-enter the other way"
        );
        b.open = HostOpen::Handover;
        assert_eq!(alt_enter_hint(&[a, b]), "   alt-enter mirror");
    }

    #[test]
    fn a_typed_name_at_a_host_is_split_only_for_a_host_that_exists() {
        let hosts = vec!["vm".to_string()];
        assert_eq!(split_at_host("api@vm", &hosts), Some(("api", "vm")));
        assert_eq!(split_at_host("a@b@vm", &hosts), Some(("a@b", "vm")));
        assert_eq!(split_at_host("api@nope", &hosts), None);
        assert_eq!(split_at_host("@vm", &hosts), None);
        assert_eq!(split_at_host("~/src/api", &hosts), None);
    }

    #[test]
    fn the_remote_line_quotes_every_name_and_runs_nothing_it_names() {
        let nasty = "x'; touch pwned; '";
        let line = remote_line(
            nasty,
            &mirror::grouped_name(nasty, "mac"),
            "mac",
            &Shim::default(),
        );
        // Run with a fake tmux that prints its argv: each name arrives as one
        // argument, and nothing in it ran.
        let dir = tempfile::tempdir().expect("tempdir");
        let tmux = dir.path().join("tmux");
        std::fs::write(
            &tmux,
            "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done; echo\n",
        )
        .expect("fake tmux");
        std::fs::set_permissions(&tmux, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let out = std::process::Command::new("sh")
            .args(["-c", &line])
            .current_dir(dir.path())
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .output()
            .expect("sh runs");
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(
            said.contains("[new-session][-A][-t][=x'; touch pwned; '][-s][x'; touch pwned; '~mac][;][set-option][@tc-mirror][mac]"),
            "{said}"
        );
        assert!(said.contains("[set-option][status][off]"), "{said}");
        assert!(
            said.contains("[set-option][destroy-unattached][on]"),
            "{said}"
        );
        assert!(!dir.path().join("pwned").exists(), "a name ran something");
        let create = remote_line(
            "api",
            "api~mac",
            "mac",
            &Shim {
                create: true,
                ..Shim::default()
            },
        );
        assert!(create.contains("new-session -d -s api"), "{create}");
        assert!(!create.contains("exit 75"), "{create}");
    }

    /// Run a remote line under `sh` with a fake `tmux` that prints its argv
    /// and a fake `tmux-companion` that names a session with a space in it.
    fn run_line(line: &str) -> (String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let printer = "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done; echo\n";
        for (name, body) in [
            ("tmux", printer),
            ("tmux-companion", "#!/bin/sh\necho 'my api'\n"),
        ] {
            let p = dir.path().join(name);
            std::fs::write(&p, body).expect("fake");
            std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .expect("chmod");
        }
        let out = std::process::Command::new("sh")
            .args(["-c", line])
            .current_dir(dir.path())
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .output()
            .expect("sh runs");
        (String::from_utf8_lossy(&out.stdout).into_owned(), dir)
    }

    #[test]
    fn a_project_on_the_host_is_started_there_and_attached_by_the_name_it_says() {
        let nasty = "/srv/x'; touch pwned; '";
        let shim = Shim {
            dir: Some(nasty.into()),
            ..Shim::default()
        };
        let line = remote_line("ignored", "ignored~mac", "mac", &shim);
        assert!(
            line.starts_with(&format!(
                "s=$(tmux-companion project --detached {}) || exit 1; ",
                crate::link::quote(nasty)
            )),
            "{line}"
        );
        let (said, dir) = run_line(&line);
        assert!(
            said.contains("[new-session][-A][-t][=my api][-s][my api~mac]"),
            "the host's name for its session, whole: {said}"
        );
        assert!(!dir.path().join("pwned").exists(), "a path ran something");
    }

    #[test]
    fn a_pane_on_the_host_is_chained_after_the_attach_and_nothing_else_is() {
        let shim = Shim {
            window: Some(2),
            pane: Some("%14".into()),
            ..Shim::default()
        };
        let line = remote_line("api", "api~mac", "mac", &shim);
        let (said, _dir) = run_line(&line);
        assert!(
            said.contains("[;][select-window][-t][:2][;][select-pane][-t][%14]"),
            "{said}"
        );
        let bad = Shim {
            pane: Some("%1; touch pwned".into()),
            ..Shim::default()
        };
        assert!(!remote_line("api", "api~mac", "mac", &bad).contains("select-pane"));
        assert!(pane_id_ok("%0") && pane_id_ok("%123"));
        for no in ["", "%", "14", "%1a", "%1 ", "%-1"] {
            assert!(!pane_id_ok(no), "{no:?}");
        }
        assert_eq!(
            shim_line("/bin/tc", "vm", "api", &shim),
            "/bin/tc host attach vm api --window 2 --pane %14"
        );
    }

    #[test]
    fn a_recorded_shim_runs_by_this_binarys_path_on_restore() {
        let recorded = recorded_shim("vm", "x'; y");
        assert_eq!(recorded, r"tmux-companion host attach vm 'x'\''; y'");
        assert_eq!(
            shim_for_restore(&format!("{recorded} --wait"), "/opt/my bin/tc"),
            r"'/opt/my bin/tc' host attach vm 'x'\''; y' --wait"
        );
        assert_eq!(shim_for_restore("nvim", "/bin/tc"), "nvim");
    }

    #[test]
    fn a_hosts_directories_are_rows_of_its_own() {
        let mut views = vec![view("vm", State::Up, &[("api", 0)])];
        views[0].sessions[0].path = "/srv/api".into();
        let rows = dir_rows(
            "vm",
            &["/srv/api".into(), "/srv/web/".into(), "/srv/web/".into()],
            &views,
        );
        assert_eq!(rows.len(), 1, "taken and repeated ones go: {rows:?}");
        assert_eq!(rows[0].label, "web@vm");
        assert_eq!(rows[0].mirror, Some(("vm".into(), "web".into())));
        assert_eq!(rows[0].kind, crate::project::Kind::Directory);
    }

    #[test]
    fn endings_are_told_apart() {
        assert_eq!(ending("vm", "api", Some(0), &[]), None);
        assert_eq!(
            ending("vm", "api", Some(GONE), &[]).as_deref(),
            Some("api no longer exists on vm")
        );
        assert_eq!(
            ending(
                "vm",
                "api",
                Some(255),
                &["ssh: connect to host vm port 22: Connection timed out".into()]
            )
            .as_deref(),
            Some(
                "the connection to vm ended: ssh: connect to host vm port 22: Connection timed out"
            )
        );
        assert_eq!(
            ending("vm", "api", Some(1), &[]).as_deref(),
            Some("the connection to vm ended: exit status 1")
        );
    }

    #[test]
    fn a_mirror_session_is_made_in_one_call_with_its_options() {
        let cmds = mirror_commands("api@vm", "vm", "api", "'tc' host attach vm api");
        assert_eq!(cmds[0][..4], ["new-session", "-d", "-s", "api@vm"]);
        let opts: Vec<(&str, &str)> = cmds[1..]
            .iter()
            .map(|c| (c[3].as_str(), c[4].as_str()))
            .collect();
        assert_eq!(
            opts,
            [
                ("@tc-host", "vm"),
                ("@tc-remote", "api"),
                ("prefix", "None"),
                ("detach-on-destroy", "previous"),
            ]
        );
        assert!(cmds[1..].iter().all(|c| c[2] == "=api@vm:"));
    }
}
