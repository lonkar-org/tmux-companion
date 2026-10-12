//! `hosts`: the other machines this daemon links to, in plain lines, and
//! `hosts add` and `hosts connect`, which add one and bring one's link up.
//!
//! The lines are the source of truth for the job "what is on my other
//! machines": one line per host in config order, then its sessions, nothing
//! said by colour or position. A picker that shows hosts is a view of these.

use std::time::Duration;

use crate::{
    config::Host,
    link::{
        Session,
        manager::{HostView, Kept, State},
    },
    proto::{HostsArgs, Request},
};

/// The tmux server option a host added without `--save` lives in, so it
/// survives a daemon restart and goes with the server it was added for.
pub const SERVER_OPTION: &str = "@tc-hosts";

/// The longest a host name may be.
pub const NAME_MAX: usize = 32;

/// Whether a name can be a host's: letters, digits, `-` and `_`, up to 32,
/// since it ends up in session names, file names under `hosts/` and shell
/// lines.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("a host needs a name".to_string());
    }
    if name.len() > NAME_MAX {
        return Err(format!(
            "`{name}` is {} characters, and a host name is at most {NAME_MAX}",
            name.len()
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
    {
        return Err(format!(
            "`{name}` has `{bad}`, and a host name is letters, digits, - and _"
        ));
    }
    Ok(())
}

/// A name for a host from its command that no host in `taken` has: the
/// command's last word that doesn't start with `-`, any `user@` dropped, cut
/// at the first `.`, every other character turned into `-` and cut to 32, so
/// `ssh dev@vm.local` gives `vm`. When that is taken it's `vm-2`, `vm-3`.
pub fn default_name(command: &[String], taken: &[String]) -> String {
    let word = command
        .iter()
        .rev()
        .find(|w| !w.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("host");
    let word = word.rsplit('@').next().unwrap_or(word);
    let word = word.split('.').next().unwrap_or(word);
    let mut base: String = word
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(NAME_MAX)
        .collect();
    if base.is_empty() {
        base = "host".to_string();
    }
    let free = |n: &str| !taken.iter().any(|t| t == n);
    if free(&base) {
        return base;
    }
    for i in 2.. {
        let suffix = format!("-{i}");
        let mut name = base.clone();
        name.truncate(NAME_MAX - suffix.len());
        name.push_str(&suffix);
        if free(&name) {
            return name;
        }
    }
    unreachable!("an unbounded range ends only by returning")
}

/// The characters a host command may not hold, and why: each would mean
/// something to the shell ssh hands the remote command to.
const REFUSED: &[(char, &str)] = &[
    ('$', "`$`"),
    ('|', "`|`"),
    (';', "`;`"),
    ('`', "a backtick"),
];

fn refuse(word: &str) -> Result<(), String> {
    for (c, said) in REFUSED {
        if word.contains(*c) {
            return Err(format!(
                "{said} isn't allowed in a host command: a shell would read it, on this machine or the host"
            ));
        }
    }
    Ok(())
}

/// A command line split into argv by shell-word rules without running a
/// shell: single quotes keep everything, double quotes keep everything but
/// `\"` and `\\`, a backslash outside quotes keeps the next character, and
/// `$`, `|`, `;` and backticks are refused wherever they are.
pub fn split_words(line: &str) -> Result<Vec<String>, String> {
    refuse(line)?;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => word.push(x),
                        None => return Err("a ' is never closed".to_string()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(x @ ('"' | '\\')) => word.push(x),
                            Some(x) => {
                                word.push('\\');
                                word.push(x);
                            }
                            None => return Err("a \" is never closed".to_string()),
                        },
                        Some(x) => word.push(x),
                        None => return Err("a \" is never closed".to_string()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(x) => word.push(x),
                    None => return Err("a \\ at the end escapes nothing".to_string()),
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    if words.is_empty() {
        return Err("the host command is empty".to_string());
    }
    Ok(words)
}

/// The argv for `hosts add`: one argument is a command line to split, more
/// are argv already, which the shell that ran `hosts add` split.
pub fn command_from(args: &[String]) -> Result<Vec<String>, String> {
    match args {
        [] => Err("the host command is empty".to_string()),
        [one] => split_words(one),
        many => {
            for w in many {
                refuse(w)?;
            }
            Ok(many.to_vec())
        }
    }
}

/// One host as `@tc-hosts` keeps it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    name: String,
    command: Vec<String>,
}

/// The hosts in `@tc-hosts`'s value: a JSON list of names and argvs, each
/// checked like a config entry. A value that doesn't parse holds none.
pub fn parse_server_hosts(text: &str) -> Vec<Host> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let Ok(stored) = serde_json::from_str::<Vec<Stored>>(text) else {
        return Vec::new();
    };
    let hosts: Vec<Host> = stored
        .into_iter()
        .map(|s| Host::new(&s.name, s.command))
        .collect();
    hosts
        .into_iter()
        .filter(|h| crate::config::check_hosts(std::slice::from_ref(h)).is_ok())
        .collect()
}

/// `@tc-hosts`'s value for a list of hosts.
pub fn render_server_hosts(hosts: &[Host]) -> String {
    let stored: Vec<Stored> = hosts
        .iter()
        .map(|h| Stored {
            name: h.name.clone(),
            command: h.command.clone(),
        })
        .collect();
    serde_json::to_string(&stored).unwrap_or_else(|_| "[]".to_string())
}

/// The `[[hosts]]` table `--save` adds to config.toml.
pub fn config_fragment(name: &str, command: &[String]) -> String {
    let mut doc = toml_edit::DocumentMut::new();
    let mut table = toml_edit::Table::new();
    table["name"] = toml_edit::value(name);
    let mut argv = toml_edit::Array::new();
    for w in command {
        argv.push(w.as_str());
    }
    table["command"] = toml_edit::value(argv);
    let mut list = toml_edit::ArrayOfTables::new();
    list.push(table);
    doc["hosts"] = toml_edit::Item::ArrayOfTables(list);
    doc.to_string()
}

/// `1 session`, `4 sessions`.
fn count(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// When the host was last heard from, in words.
fn heard(heard_at: u64, now: u64) -> String {
    if heard_at == 0 {
        "never heard from".to_string()
    } else {
        format!(
            "last heard {}",
            crate::sessions::idle::age(now.saturating_sub(heard_at))
        )
    }
}

/// One host as the brief says it: `vm: 4 sessions, 1 agent asked, last
/// heard 12s`, or `vm: 4 sessions, last heard 3d, unreachable: Connection
/// timed out`. From what the daemon holds and nothing newer: the agents are
/// there only while the link is up.
pub fn brief_line(v: &HostView, now: u64) -> String {
    let mut parts = Vec::new();
    if v.heard_at == 0 && v.sessions.is_empty() {
        parts.push("never heard from".to_string());
    } else {
        parts.push(count(v.sessions.len(), "session"));
        let asked = v.inbox.iter().filter(|e| e.state != "done").count();
        if asked > 0 {
            parts.push(format!("{} asked", count(asked, "agent")));
        }
        if v.heard_at != 0 {
            parts.push(heard(v.heard_at, now));
        }
    }
    match &v.state {
        State::Down { reason, .. } => parts.push(format!("unreachable: {reason}")),
        State::Off => parts.push("link off".to_string()),
        State::Idle | State::Connecting | State::Up => {}
    }
    format!("{}: {}", v.name, parts.join(", "))
}

/// One host as a line: its name, mode, where the link is and why, when it
/// was last heard from, how many sessions, its build and tmux, and anything
/// that leaves a feature out for it.
pub fn line(v: &HostView, now: u64) -> String {
    let mut parts = vec![format!("{}: {}", v.name, v.link.word())];
    match &v.state {
        State::Off => {}
        State::Idle => parts.push("no link".to_string()),
        State::Connecting => parts.push("connecting".to_string()),
        State::Up => {
            parts.push("link up".to_string());
            parts.push(if v.pushing {
                "pushing".to_string()
            } else {
                format!("polled every {}s", v.poll_secs)
            });
        }
        State::Down { reason, retry_at } => {
            let wait = retry_at.saturating_sub(now);
            if wait > 0 {
                parts.push(format!(
                    "link down: {reason}, retrying in {}",
                    crate::sessions::idle::age(wait)
                ));
            } else {
                parts.push(format!("link down: {reason}"));
            }
        }
    }
    parts.push(heard(v.heard_at, now));
    if v.heard_at != 0 {
        parts.push(count(v.sessions.len(), "session"));
    }
    if !v.build.is_empty() {
        parts.push(format!("tmux-companion {}", v.build));
    }
    if !v.tmux.is_empty() {
        parts.push(v.tmux.clone());
        if !crate::setup::catalog::at_least(&v.tmux, (3, 4)) {
            parts.push("tmux older than 3.4, the floor".to_string());
        }
    }
    if !v.lacks.is_empty() {
        parts.push(format!("its bridge lacks {}", v.lacks.join(" and ")));
    }
    if v.started_daemon {
        parts.push("the bridge started its daemon".to_string());
    }
    if v.kept == Kept::Server {
        parts.push("kept until this tmux server exits".to_string());
    }
    parts.join(", ")
}

/// One of a host's sessions as a line, the host in words after the name.
pub fn session_line(s: &Session, host: &str) -> String {
    let mut line = format!("{}@{host}, {}", s.name, count(s.windows as usize, "window"));
    if s.attached > 0 {
        line.push_str(&format!(", attached on {host}"));
    }
    line
}

/// Every host and its sessions, in order: each host's line, then its
/// sessions in the host's own recency.
pub fn lines(views: &[HostView], now: u64) -> String {
    let mut out = String::new();
    for v in views {
        out.push_str(&line(v, now));
        out.push('\n');
        for s in &v.sessions {
            out.push_str(&session_line(s, &v.name));
            out.push('\n');
        }
    }
    out
}

/// The `hosts` line in `doctor`: each host's line, joined, or `none`.
pub fn doctor_line(views: &[HostView], now: u64) -> String {
    if views.is_empty() {
        return "none".to_string();
    }
    views
        .iter()
        .map(|v| line(v, now))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Ask the daemon for every host, after an ask or a reload.
async fn ask(args: &HostsArgs) -> anyhow::Result<Vec<HostView>> {
    let resp = crate::client::send(Request::build("__hosts", args)).await?;
    if let Some(e) = resp.error {
        anyhow::bail!("{e}");
    }
    Ok(serde_json::from_str(&resp.output)?)
}

/// `doctor`'s value, from a daemon that is already running; it starts none.
pub fn doctor_state() -> String {
    let req = Request::build("__hosts", &HostsArgs::default());
    match crate::client::ask_running(&req, Duration::from_millis(500)) {
        Some(out) => match serde_json::from_str::<Vec<HostView>>(&out) {
            Ok(views) => doctor_line(&views, crate::panes::now_secs()),
            Err(_) => "the daemon's answer didn't parse".to_string(),
        },
        None => {
            let n = crate::config::load()
                .map(|(c, _)| c.hosts.len())
                .unwrap_or(0);
            if n == 0 {
                "none".to_string()
            } else {
                format!("{} in config, no daemon to ask", count(n, "host"))
            }
        }
    }
}

/// What `hosts` can do besides list.
#[derive(clap::Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum HostsAction {
    /// Add a host: `hosts add vm -- ssh vm`. Kept until this tmux server
    /// exits, or in config.toml with --save. With neither, in a terminal, a
    /// form asks for both and tests the command first
    Add {
        /// The word on every row: letters, digits, - and _, up to 32
        name: Option<String>,
        /// Write it to config.toml as a `[[hosts]]` entry
        #[arg(long)]
        save: bool,
        /// The command that opens ssh to it, after `--`
        #[arg(last = true, value_name = "COMMAND")]
        command: Vec<String>,
    },
    /// Start or retry a host's link now and say how it went, one line per
    /// host
    Connect {
        /// The host; every host without one
        name: Option<String>,
    },
    /// Forget a host: its cached sessions, and its `[[hosts]]` entry in
    /// config.toml or its place on this tmux server. Refused while a mirror
    /// session to it is open
    Forget {
        /// The host
        name: String,
        /// Internal: the key pressed at `forget vm? y/n`; only `y` forgets
        #[arg(long, hide = true)]
        answer: Option<String>,
        /// Internal: the client the question was asked on
        #[arg(long, hide = true)]
        client: Option<String>,
        /// Internal: open the session switcher again afterwards
        #[arg(long, hide = true)]
        reopen: bool,
    },
}

/// `hosts`, `hosts add` and `hosts connect`.
pub async fn run(action: Option<HostsAction>) -> anyhow::Result<()> {
    match action {
        None => {
            let views = ask(&HostsArgs::default()).await?;
            if views.is_empty() {
                println!("no hosts: add one with tmux-companion hosts add NAME -- ssh NAME");
            } else {
                print!("{}", lines(&views, crate::panes::now_secs()));
            }
            Ok(())
        }
        Some(HostsAction::Add {
            name: None,
            save: _,
            command,
        }) if command.is_empty() => {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() {
                anyhow::bail!("hosts add needs a name and a command: hosts add vm -- ssh vm");
            }
            if let Some(said) = form().await? {
                println!("{said}");
                // From a popup, which closes with the form.
                if std::env::var_os("TMUX").is_some() {
                    crate::remote::say(&said).await;
                }
            }
            Ok(())
        }
        Some(HostsAction::Add {
            name: Some(name),
            save,
            command,
        }) if !command.is_empty() => add(&name, save, &command).await,
        Some(HostsAction::Add { .. }) => {
            anyhow::bail!("hosts add needs a name and a command: hosts add vm -- ssh vm")
        }
        Some(HostsAction::Connect { name }) => connect(name).await,
        Some(HostsAction::Forget {
            name,
            answer,
            client,
            reopen,
        }) => forget_command(&name, answer, client, reopen).await,
    }
}

/// `hosts add`.
async fn add(name: &str, save: bool, args: &[String]) -> anyhow::Result<()> {
    let command = command_from(args).map_err(anyhow::Error::msg)?;
    let said = add_host(name, save, &command).await?;
    println!("{said}");
    Ok(())
}

/// The hosts already known, config first, then the tmux server's.
async fn known() -> anyhow::Result<Vec<Host>> {
    let mut all = crate::config::load()
        .map(|(c, _)| c.hosts)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for h in crate::link::manager::server_hosts().await {
        if !all.iter().any(|x| x.name == h.name) {
            all.push(h);
        }
    }
    Ok(all)
}

/// The line refusing a name already in use, naming that host's command.
fn taken_line(h: &Host) -> String {
    format!(
        "{} is already a host, reached with {}",
        h.name,
        h.command
            .iter()
            .map(|w| crate::link::quote(w))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// Add a host and say where it was kept, in one line: `--save` writes it to
/// config.toml, otherwise it goes in `@tc-hosts` until the tmux server
/// exits. The daemon reads its hosts again after either.
pub async fn add_host(name: &str, save: bool, command: &[String]) -> anyhow::Result<String> {
    check_name(name).map_err(anyhow::Error::msg)?;
    for w in command {
        refuse(w).map_err(anyhow::Error::msg)?;
    }
    let config_hosts = crate::config::load()
        .map(|(c, _)| c.hosts)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let server = crate::link::manager::server_hosts().await;
    if let Some(h) = config_hosts
        .iter()
        .chain(server.iter())
        .find(|h| h.name == name)
    {
        anyhow::bail!("{}", taken_line(h));
    }
    let said = if save {
        let path = crate::setup::config_file()?;
        let old = std::fs::read_to_string(&path).ok();
        let text = crate::setup::merge::merged(
            old.as_deref().unwrap_or(""),
            &config_fragment(name, command),
            None,
        )?;
        write_config(&path, old.as_deref(), &text)?;
        format!("{name} saved to {}", path.display())
    } else {
        let mut all = server;
        all.push(Host::new(name, command.to_vec()));
        set_server_hosts(&all, name).await?;
        format!("{name} kept until this tmux server exits; --save keeps it in config.toml")
    };
    ask(&HostsArgs {
        reload: true,
        ..HostsArgs::default()
    })
    .await?;
    Ok(said)
}

/// Write config.toml through any symlink to the real file, and put it back
/// as it was when the result doesn't load.
fn write_config(path: &std::path::Path, old: Option<&str>, text: &str) -> anyhow::Result<()> {
    let landed = crate::setup::block::write_through(path, text)?;
    if let Err(e) = crate::config::load_from(path) {
        match old {
            Some(t) => {
                let _ = crate::setup::block::write_through(path, t);
            }
            None => {
                let _ = std::fs::remove_file(&landed);
            }
        }
        anyhow::bail!("{e}\nso {} is back as it was", path.display());
    }
    Ok(())
}

/// Replace `@tc-hosts` with `hosts`, or unset it when there are none.
async fn set_server_hosts(hosts: &[Host], name: &str) -> anyhow::Result<()> {
    let out = if hosts.is_empty() {
        crate::tmux::command()
            .args(["set-option", "-su", SERVER_OPTION])
            .output()
            .await?
    } else {
        crate::tmux::command()
            .args([
                "set-option",
                "-s",
                SERVER_OPTION,
                &render_server_hosts(hosts),
            ])
            .output()
            .await?
    };
    if !out.status.success() {
        anyhow::bail!(
            "no tmux server to keep {name} on ({}); --save writes it to config.toml instead",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// config.toml's text without the `[[hosts]]` entry called `name`, comments
/// and everything else kept; `None` when it has no such entry.
pub fn without_host(text: &str, name: &str) -> anyhow::Result<Option<String>> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let Some(item) = doc.get_mut("hosts") else {
        return Ok(None);
    };
    let Some(list) = item.as_array_of_tables_mut() else {
        return Ok(None);
    };
    let before = list.len();
    list.retain(|t| t.get("name").and_then(|v| v.as_str()) != Some(name));
    if list.len() == before {
        return Ok(None);
    }
    if list.is_empty() {
        doc.remove("hosts");
    }
    Ok(Some(doc.to_string()))
}

/// The mirror sessions open on this tmux server for `host`, by name.
async fn open_mirrors(host: &str) -> Vec<String> {
    let format = format!("#{{{}}}\t#{{session_name}}", crate::link::HOST_OPTION);
    crate::cli::tmux_capture(&["list-sessions", "-F", &format])
        .await
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(h, _)| *h == host)
        .map(|(_, s)| s.to_string())
        .collect()
}

/// Forget a host: refused while a mirror session to it is open, otherwise
/// its entry goes from config.toml or `@tc-hosts`, the daemon reads its
/// hosts again, and its cache file goes. One line saying what happened.
pub async fn forget(name: &str) -> anyhow::Result<String> {
    if let Some(open) = open_mirrors(name).await.first() {
        anyhow::bail!("{open} is open; close it first");
    }
    let path = crate::setup::config_file()?;
    let old = std::fs::read_to_string(&path).ok();
    let from_config = match old.as_deref() {
        Some(text) => without_host(text, name)?,
        None => None,
    };
    let said = if let Some(text) = from_config {
        write_config(&path, old.as_deref(), &text)?;
        format!("forgot {name}, and its entry in {}", path.display())
    } else {
        let server = crate::link::manager::server_hosts().await;
        if !server.iter().any(|h| h.name == name) {
            anyhow::bail!("no host called {name}; `hosts` lists them");
        }
        let rest: Vec<Host> = server.into_iter().filter(|h| h.name != name).collect();
        set_server_hosts(&rest, name).await?;
        format!("forgot {name}")
    };
    ask(&HostsArgs {
        reload: true,
        ..HostsArgs::default()
    })
    .await?;
    if let Some(dir) = crate::link::cache::dir() {
        crate::link::cache::remove(&crate::link::cache::path_in(&dir, name));
    }
    Ok(said)
}

/// `hosts forget`: from a terminal, the line on stdout; from the question
/// on tmux's message line, the line there, and the switcher open again.
async fn forget_command(
    name: &str,
    answer: Option<String>,
    client: Option<String>,
    reopen: bool,
) -> anyhow::Result<()> {
    let result = match answer.as_deref() {
        Some("y") | None => forget(name).await,
        Some(_) => Ok(format!("kept {name}")),
    };
    if answer.is_none() && !reopen {
        return result.map(|said| println!("{said}"));
    }
    let said = match result {
        Ok(s) => s,
        Err(e) => format!("{e:#}"),
    };
    let mut msg = vec!["display-message".to_string()];
    if let Some(c) = &client {
        msg.extend(["-c".to_string(), c.clone()]);
    }
    msg.push(crate::picker::literal_for_tmux(&said));
    let _ = crate::tmux::command().args(&msg).output().await;
    if reopen {
        reopen_switcher(client.as_deref(), &format!("host:{name}")).await?;
    }
    Ok(())
}

/// Open `switch sessions` in a popup again, on the row `at`: what follows a
/// question asked on the message line, which needed the popup closed.
pub async fn reopen_switcher(client: Option<&str>, at: &str) -> anyhow::Result<()> {
    let version = crate::cli::tmux_capture(&["display-message", "-p", "#{version}"]).await;
    let exe = std::env::current_exe()?.display().to_string();
    let inner = format!(
        "{} switch sessions --at {}",
        crate::link::quote(&exe),
        crate::link::quote(at)
    );
    let mut args = vec!["display-popup".to_string()];
    if let Some(c) = client {
        args.extend(["-c".to_string(), c.to_string()]);
    }
    if crate::switcher::borderless(version.trim()) {
        args.push("-B".to_string());
    }
    args.extend(["-E", "-w", "80%", "-h", "70%"].map(String::from));
    args.push(inner);
    // Not waited for: the popup holds its command client until it closes.
    crate::tmux::command_sync()
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

/// A shell line as a double-quoted tmux string, for a command tmux parses.
fn tmux_quoted(line: &str) -> String {
    let mut out = String::from("\"");
    for c in line.chars() {
        if matches!(c, '"' | '\\' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The `command-prompt` that asks `forget vm? y/n` on tmux's message line
/// and runs `hosts forget` with the key pressed, then opens the switcher
/// again on the row it was on. One key answers: `-1`.
pub fn forget_prompt(exe: &str, host: &str) -> Vec<String> {
    let line = format!(
        "{} hosts forget {} --answer=%1 --client '#{{client_name}}' --reopen",
        crate::link::quote(exe),
        crate::link::quote(host)
    );
    vec![
        "command-prompt".to_string(),
        "-1".to_string(),
        "-p".to_string(),
        format!("forget {host}? y/n"),
        format!("run-shell -b {}", tmux_quoted(&line)),
    ]
}

/// Ask `forget vm? y/n` on tmux's message line, for ctrl-x on a host's line
/// in the switcher, which closes for the question.
pub async fn ask_to_forget(host: &str) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?.display().to_string();
    crate::tmux::command()
        .args(forget_prompt(&exe, host))
        .output()
        .await?;
    Ok(())
}

/// The add-host form's fields and what it last said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    /// The name field.
    pub name: String,
    /// Whether the name is the person's own rather than the default.
    pub named: bool,
    /// The command field, as typed.
    pub command: String,
    /// Which field the cursor is in: false for the name, true for the
    /// command.
    pub on_command: bool,
    /// The one line under the fields: a test's answer, or why not.
    pub said: String,
    /// The command answered a test, or was tested and refused: `s` and `t`
    /// act now, and any other key goes back to editing.
    pub tested: bool,
    /// How the last thing said went: answered (true), refused or failed
    /// (false), or neither, such as `connecting`. Drawn as a tone; the words
    /// say it too.
    pub passed: Option<bool>,
}

/// What a key did to the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAct {
    /// Nothing to do but draw.
    Edit,
    /// Test the command with a hello.
    Test(String, Vec<String>),
    /// Keep the host: in config.toml (true), or until the tmux server exits.
    Keep(bool),
    /// Leave without adding anything.
    Leave,
}

impl Form {
    /// The name the form shows: the person's, or the default from the
    /// command once it splits.
    fn refresh_name(&mut self, taken: &[String]) {
        if self.named {
            return;
        }
        if let Ok(words) = split_words(&self.command) {
            self.name = default_name(&words, taken);
        } else if self.command.trim().is_empty() {
            self.name.clear();
        }
    }

    /// One key. `taken` holds every host's name, with the command each is
    /// reached with, for the line refusing a name in use.
    pub fn key(&mut self, key: ratatui::crossterm::event::KeyEvent, taken: &[Host]) -> FormAct {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let names: Vec<String> = taken.iter().map(|h| h.name.clone()).collect();
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return FormAct::Leave;
        }
        if self.tested {
            match key.code {
                KeyCode::Char('s') => return FormAct::Keep(true),
                KeyCode::Char('t') => return FormAct::Keep(false),
                _ => {
                    self.tested = false;
                    self.said.clear();
                    self.passed = None;
                }
            }
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                self.on_command = !self.on_command;
            }
            KeyCode::Enter => {
                let command = match split_words(&self.command) {
                    Ok(c) => c,
                    Err(e) => {
                        self.said = e;
                        self.passed = Some(false);
                        self.on_command = true;
                        return FormAct::Edit;
                    }
                };
                if let Err(e) = check_name(&self.name) {
                    self.said = e;
                    self.passed = Some(false);
                    self.on_command = false;
                    return FormAct::Edit;
                }
                if let Some(h) = taken.iter().find(|h| h.name == self.name) {
                    self.said = taken_line(h);
                    self.passed = Some(false);
                    self.on_command = false;
                    return FormAct::Edit;
                }
                self.said = format!("{}: connecting", self.name);
                self.passed = None;
                return FormAct::Test(self.name.clone(), command);
            }
            KeyCode::Backspace => {
                if self.on_command {
                    self.command.pop();
                } else {
                    self.name.pop();
                    self.named = true;
                }
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if self.on_command {
                    self.command.push(c);
                } else {
                    self.name.push(c);
                    self.named = true;
                }
            }
            _ => {}
        }
        self.refresh_name(&names);
        FormAct::Edit
    }

    /// The screen, as plain lines, and where the cursor goes on it (line,
    /// column, both from 0): in the field being typed into, or at the end of
    /// the answer once there is one to read.
    pub fn screen(&self) -> (Vec<String>, (usize, usize)) {
        let keys = if self.tested {
            "s save to config.toml   t keep for this tmux server   esc leave   any other key edits"
        } else {
            "tab next field   enter test the command   esc leave"
        };
        // The keys first, where every picker has them, then the fields.
        let lines = vec![
            keys.to_string(),
            "Add a host".to_string(),
            format!("name: {}", self.name),
            format!("command: {}", self.command),
            self.said.clone(),
        ];
        let at = if self.tested {
            (4, self.said.chars().count())
        } else if self.on_command {
            (3, "command: ".len() + self.command.chars().count())
        } else {
            (2, "name: ".len() + self.name.chars().count())
        };
        (lines, at)
    }
}

/// Draw the form in one write: the whole screen, then the cursor. The keys
/// are drawn the way a picker's hint is, keys strong and words dim, with the
/// picker's rule under them; the answer to a test is in the `Ok` tone when the
/// host answered and `Failed` when it didn't, and its words say which.
fn draw_form(f: &Form) -> std::io::Result<()> {
    use crate::picker::paint::{Paint, Tone, hint_parts};
    use std::io::Write;
    // The stdout paint: `detect` alone is the pickers', whose escapes are off
    // because ratatui draws their styles, and the form prints its own.
    let paint = Paint::for_stdout(false);
    let (lines, (row, col)) = f.screen();
    let width = ratatui::crossterm::terminal::size().map_or(80, |(w, _)| w as usize);
    let mut out = String::from("\x1b[H\x1b[2J ");
    for (word, key) in hint_parts(&lines[0]) {
        out.push_str(&paint.ink(&word, if key { Tone::Strong } else { Tone::Dim }));
    }
    out.push_str("\r\n ");
    out.push_str(&paint.ink(&"─".repeat(width.saturating_sub(2)), Tone::Dim));
    out.push_str("\r\n");
    let said_tone = match f.passed {
        Some(true) => Tone::Ok,
        Some(false) => Tone::Failed,
        None => Tone::Dim,
    };
    let body: Vec<String> = lines[1..]
        .iter()
        .enumerate()
        .map(|(i, l)| match i {
            0 => format!(" {}", paint.ink(l, Tone::Strong)),
            3 => format!(" {}", paint.ink(l, said_tone)),
            _ => format!(" {l}"),
        })
        .collect();
    out.push_str(&body.join("\r\n"));
    // One row more for the rule, one column for the inset.
    out.push_str(&format!("\x1b[{};{}H", row + 2, col + 2));
    let mut stdout = std::io::stdout();
    stdout.write_all(out.as_bytes())?;
    stdout.flush()
}

/// Say hello to a host that isn't kept yet, once, and say how it went.
async fn test_hello(name: &str, command: Vec<String>) -> (String, bool) {
    let host = Host::new(name, command);
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            match crate::link::conn::open(&host).await {
                Ok((link, hello)) => {
                    link.close().await;
                    let mut said = format!("{name} answered: tmux-companion {}", hello.build);
                    if !hello.tmux.is_empty() {
                        said.push_str(&format!(", {}", hello.tmux));
                    }
                    (said, true)
                }
                Err(reason)
                    if reason
                        .strip_prefix(name)
                        .is_some_and(|rest| rest.starts_with([' ', ':'])) =>
                {
                    (reason, false)
                }
                Err(reason) => (format!("{name}: {reason}"), false),
            }
        })
        .await
}

/// The add-host form, in this terminal: two labelled fields, name and
/// command, Tab between them, Enter tests the command with a hello, then `s`
/// keeps it in config.toml or `t` for this tmux server, and Esc leaves. The
/// name is filled in from the command until the person types their own.
/// Every state is one line. Returns the line saying where the host was kept,
/// or `None` when nothing was added.
pub async fn form() -> anyhow::Result<Option<String>> {
    use ratatui::crossterm::event::{self, Event, KeyEventKind};
    let taken = known().await?;
    let mut f = Form::default();
    ratatui::crossterm::terminal::enable_raw_mode()?;
    let result: anyhow::Result<Option<String>> = async {
        loop {
            draw_form(&f)?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match f.key(key, &taken) {
                FormAct::Edit => {}
                FormAct::Leave => return Ok(None),
                FormAct::Test(name, command) => {
                    draw_form(&f)?;
                    let (said, ok) = test_hello(&name, command).await;
                    f.said = said;
                    f.passed = Some(ok);
                    f.tested = true;
                }
                FormAct::Keep(save) => {
                    let command = split_words(&f.command).map_err(anyhow::Error::msg)?;
                    match add_host(&f.name, save, &command).await {
                        Ok(said) => return Ok(Some(said)),
                        Err(e) => {
                            f.said = format!("{e:#}");
                            f.passed = Some(false);
                            f.tested = false;
                        }
                    }
                }
            }
        }
    }
    .await;
    let _ = ratatui::crossterm::terminal::disable_raw_mode();
    print!("\x1b[H\x1b[2J");
    result
}

/// The outcome of a connect, as one line: what answered, or why not.
pub fn outcome(v: &HostView) -> Option<(String, bool)> {
    match &v.state {
        State::Up => {
            let mut said = format!("{} answered: tmux-companion {}", v.name, v.build);
            if !v.tmux.is_empty() {
                said.push_str(&format!(", {}", v.tmux));
            }
            Some((said, true))
        }
        // A reason that starts with the host's name says it already.
        State::Down { reason, .. }
            if reason
                .strip_prefix(v.name.as_str())
                .is_some_and(|rest| rest.starts_with([' ', ':'])) =>
        {
            Some((reason.clone(), false))
        }
        State::Down { reason, .. } => Some((format!("{}: {reason}", v.name), false)),
        State::Off => Some((format!("{}: its link is off", v.name), false)),
        State::Idle | State::Connecting => None,
    }
}

/// `hosts connect`: ask, then wait for each host's attempt to end, up to its
/// connect time and a little more, and say how each went.
async fn connect(name: Option<String>) -> anyhow::Result<()> {
    let before = ask(&HostsArgs {
        connect: Some(name.clone().unwrap_or_default()),
        ..HostsArgs::default()
    })
    .await?;
    let targets: Vec<HostView> = before
        .into_iter()
        .filter(|v| name.as_deref().is_none_or(|n| n == v.name))
        .collect();
    if targets.is_empty() {
        match name {
            Some(n) => anyhow::bail!("no host called {n}; `hosts` lists them"),
            None => {
                anyhow::bail!("no hosts: add one with tmux-companion hosts add NAME -- ssh NAME")
            }
        }
    }
    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
    let longest = targets
        .iter()
        .map(|v| {
            config
                .hosts
                .iter()
                .find(|h| h.name == v.name)
                .map_or(10, |h| h.connect_secs)
        })
        .max()
        .unwrap_or(10);
    let deadline = std::time::Instant::now() + Duration::from_secs(longest + 3);
    let mut all_ok = true;
    for t in &targets {
        let line = loop {
            let now = ask(&HostsArgs::default()).await?;
            let Some(v) = now.into_iter().find(|v| v.name == t.name) else {
                break (format!("{}: gone", t.name), false);
            };
            let settled = v.state == State::Off
                || v.attempts > t.attempts
                || (t.state == State::Up && v.state == State::Up);
            if settled && let Some(o) = outcome(&v) {
                break o;
            }
            if std::time::Instant::now() > deadline {
                break (format!("{}: still connecting", t.name), false);
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        };
        all_ok &= line.1;
        println!("{}", line.0);
    }
    if !all_ok {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LinkMode;

    #[test]
    fn names_are_letters_digits_dashes_and_underscores_up_to_32() {
        assert!(check_name("vm").is_ok());
        assert!(check_name("dev_box-2").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name("a.b").is_err());
        assert!(check_name("x'y").is_err());
        assert!(check_name(&"a".repeat(32)).is_ok());
        assert!(check_name(&"a".repeat(33)).is_err());
    }

    #[test]
    fn the_default_name_comes_from_the_command_and_never_collides() {
        let c = |s: &str| split_words(s).expect("splits");
        assert_eq!(default_name(&c("ssh dev@vm.local"), &[]), "vm");
        assert_eq!(default_name(&c("ssh -p 2222 box"), &[]), "box");
        assert_eq!(default_name(&c("ssh vm --"), &[]), "vm", "flags skipped");
        assert_eq!(default_name(&c("ssh 'we ird'"), &[]), "we-ird");
        let taken = vec!["vm".to_string(), "vm-2".to_string()];
        assert_eq!(default_name(&c("ssh vm"), &taken), "vm-3");
        let long = "a".repeat(40);
        let name = default_name(std::slice::from_ref(&long), &[long[..32].to_string()]);
        assert_eq!(name.len(), 32);
        assert!(name.ends_with("-2"));
        assert!(check_name(&name).is_ok());
    }

    #[test]
    fn a_command_line_splits_like_a_shell_and_refuses_what_a_shell_would_run() {
        assert_eq!(
            split_words(r#"ssh -o 'Opt=a b' "dev@vm" x\ y"#).expect("splits"),
            ["ssh", "-o", "Opt=a b", "dev@vm", "x y"]
        );
        assert_eq!(split_words(r#""a\"b""#).expect("splits"), ["a\"b"]);
        for bad in ["ssh vm; rm -rf ~", "ssh $HOST", "ssh `id`", "ssh vm | cat"] {
            assert!(split_words(bad).is_err(), "{bad} refused");
        }
        assert!(split_words("ssh 'vm").is_err(), "unclosed");
        assert!(split_words("   ").is_err(), "empty");
        assert_eq!(
            command_from(&["ssh".into(), "dev@vm".into()]).expect("argv"),
            ["ssh", "dev@vm"]
        );
        assert!(command_from(&["ssh".into(), "$x".into()]).is_err());
        assert_eq!(
            command_from(&["ssh vm".into()]).expect("split"),
            ["ssh", "vm"]
        );
    }

    #[test]
    fn hosts_kept_on_the_server_round_trip_and_bad_ones_are_dropped() {
        let hosts = vec![
            Host::new("vm", vec!["ssh".into(), "vm".into()]),
            Host::new("b", vec!["env".into(), "X=1".into()]),
        ];
        let text = render_server_hosts(&hosts);
        assert_eq!(parse_server_hosts(&text), hosts);
        assert!(parse_server_hosts("").is_empty());
        assert!(parse_server_hosts("not json").is_empty());
        assert_eq!(
            parse_server_hosts(
                r#"[{"name":"a b","command":["ssh"]},{"name":"ok","command":["ssh"]}]"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn the_saved_fragment_is_a_hosts_entry_config_reads_back() {
        let frag = config_fragment("vm", &["ssh".into(), "dev@vm \"x\"".into()]);
        let c = crate::config::parse(&frag, std::path::Path::new("x.toml")).expect("parses");
        assert_eq!(
            c.hosts,
            [Host::new("vm", vec!["ssh".into(), "dev@vm \"x\"".into()])]
        );
    }

    fn view(state: State) -> HostView {
        HostView {
            name: "vm".into(),
            command: vec!["ssh".into(), "vm".into()],
            link: LinkMode::OnDemand,
            kept: Kept::Config,
            state,
            heard_at: 0,
            build: String::new(),
            tmux: String::new(),
            started_daemon: false,
            lacks: Vec::new(),
            attempts: 0,
            sessions: Vec::new(),
            hello_ms: 0,
            tried_at: 0,
            inbox: Vec::new(),
            open: crate::config::HostOpen::Mirror,
            pushing: false,
            poll_secs: 5,
        }
    }

    #[test]
    fn the_brief_says_each_host_in_one_line() {
        let now = 1_000_000;
        assert_eq!(brief_line(&view(State::Idle), now), "vm: never heard from");
        let mut up = view(State::Up);
        up.heard_at = now - 12;
        up.sessions = vec![Session::default(); 4];
        let agent = |state: &str| crate::inbox::Entry {
            id: "%1".into(),
            at: "api:1.0".into(),
            program: "claude".into(),
            path: String::new(),
            since: 0,
            lines: String::new(),
            state: state.into(),
            question: String::new(),
            host: "vm".into(),
        };
        up.inbox = vec![agent("asked"), agent("done")];
        assert_eq!(
            brief_line(&up, now),
            "vm: 4 sessions, 1 agent asked, last heard 12s"
        );
        let mut down = view(State::Down {
            reason: "Connection timed out".into(),
            retry_at: now + 20,
        });
        down.heard_at = now - 3 * 86_400;
        down.sessions = vec![Session::default(); 4];
        assert_eq!(
            brief_line(&down, now),
            "vm: 4 sessions, last heard 3d, unreachable: Connection timed out"
        );
    }

    #[test]
    fn a_host_line_says_everything_in_words() {
        let now = 1_000_000;
        assert_eq!(
            line(&view(State::Idle), now),
            "vm: on-demand, no link, never heard from"
        );
        let mut up = view(State::Up);
        up.heard_at = now - 12;
        up.build = "0.10.0".into();
        up.tmux = "tmux 3.3a".into();
        up.lacks = vec!["segments".into(), "inbox".into()];
        up.started_daemon = true;
        up.kept = Kept::Server;
        up.sessions = vec![Session {
            name: "api".into(),
            windows: 3,
            attached: 1,
            ..Session::default()
        }];
        assert_eq!(
            line(&up, now),
            "vm: on-demand, link up, polled every 5s, last heard 12s, 1 session, \
             tmux-companion 0.10.0, tmux 3.3a, tmux older than 3.4, the floor, \
             its bridge lacks segments and inbox, the bridge started its daemon, \
             kept until this tmux server exits"
        );
        let mut pushed = view(State::Up);
        pushed.pushing = true;
        pushed.heard_at = now - 2;
        assert_eq!(
            line(&pushed, now),
            "vm: on-demand, link up, pushing, last heard 2s, 0 sessions"
        );
        assert!(
            !line(&view(State::Idle), now).contains("polled"),
            "a link that isn't up is neither"
        );
        let mut down = view(State::Down {
            reason: "no answer in 10s".into(),
            retry_at: now + 20,
        });
        down.heard_at = now - 3 * 86_400;
        assert_eq!(
            line(&down, now),
            "vm: on-demand, link down: no answer in 10s, retrying in 20s, last heard 3d, 0 sessions"
        );
        assert_eq!(
            lines(&[up], now).lines().nth(1),
            Some("api@vm, 3 windows, attached on vm")
        );
    }

    #[test]
    fn forgetting_a_saved_host_keeps_everything_else_in_the_file() {
        let text = "# mine\n[bar]\nmode = \"push\"\n\n[[hosts]]\nname = \"vm\"\ncommand = [\"ssh\", \"vm\"]\n\n[[hosts]]\nname = \"box\" # the other one\ncommand = [\"ssh\", \"box\"]\n";
        let out = without_host(text, "vm").expect("parses").expect("found");
        assert!(out.contains("# mine"), "{out}");
        assert!(out.contains("# the other one"), "{out}");
        assert!(!out.contains("\"vm\""), "{out}");
        let c = crate::config::parse(&out, std::path::Path::new("x.toml")).expect("still a config");
        assert_eq!(c.hosts.len(), 1);
        let last = without_host(&out, "box").expect("parses").expect("found");
        assert!(!last.contains("[[hosts]]"), "{last}");
        assert_eq!(without_host(text, "nope").expect("parses"), None);
    }

    #[test]
    fn the_forget_question_runs_the_answer_through_one_key_and_reopens() {
        let p = forget_prompt("/opt/tc bin/tmux-companion", "vm");
        assert_eq!(p[..4], ["command-prompt", "-1", "-p", "forget vm? y/n"]);
        assert_eq!(
            p[4],
            "run-shell -b \"'/opt/tc bin/tmux-companion' hosts forget vm --answer=%1 --client '#{client_name}' --reopen\""
        );
    }

    #[test]
    fn the_form_names_the_host_from_its_command_until_it_is_named() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
        let taken = vec![Host::new("vm", vec!["ssh".into(), "vm".into()])];
        let mut f = Form::default();
        f.key(k(KeyCode::Tab), &taken);
        for c in "ssh dev@vm.local".chars() {
            f.key(k(KeyCode::Char(c)), &taken);
        }
        assert_eq!(f.name, "vm-2", "vm is taken");
        let (lines, at) = f.screen();
        assert_eq!(
            lines[0],
            "tab next field   enter test the command   esc leave"
        );
        assert_eq!(lines[2], "name: vm-2");
        assert_eq!(lines[3], "command: ssh dev@vm.local");
        assert_eq!(
            at,
            (3, "command: ssh dev@vm.local".len()),
            "the cursor is in the field"
        );
        assert_eq!(
            f.key(k(KeyCode::Enter), &taken),
            FormAct::Test("vm-2".into(), vec!["ssh".into(), "dev@vm.local".into()])
        );
        assert_eq!(f.said, "vm-2: connecting");
        f.tested = true;
        assert_eq!(f.key(k(KeyCode::Char('s')), &taken), FormAct::Keep(true));
        // Any other key edits again.
        f.key(k(KeyCode::Char('x')), &taken);
        assert!(!f.tested);
        assert_eq!(f.command, "ssh dev@vm.localx");
        // A name typed over the default is the person's, and a taken one is
        // refused with the command it is reached with.
        f.key(k(KeyCode::Tab), &taken);
        for _ in 0..4 {
            f.key(k(KeyCode::Backspace), &taken);
        }
        for c in "vm".chars() {
            f.key(k(KeyCode::Char(c)), &taken);
        }
        assert_eq!(f.key(k(KeyCode::Enter), &taken), FormAct::Edit);
        assert_eq!(f.said, "vm is already a host, reached with ssh vm");
        f.key(k(KeyCode::Tab), &taken);
        f.command = "ssh $HOST".into();
        assert_eq!(f.key(k(KeyCode::Enter), &taken), FormAct::Edit);
        assert!(f.said.contains("`$` isn't allowed"), "{}", f.said);
        assert_eq!(f.key(k(KeyCode::Esc), &taken), FormAct::Leave);
    }

    #[test]
    fn a_connect_settles_on_up_down_or_off() {
        let mut up = view(State::Up);
        up.build = "1.0".into();
        up.tmux = "tmux 3.4".into();
        assert_eq!(
            outcome(&up),
            Some((
                "vm answered: tmux-companion 1.0, tmux 3.4".to_string(),
                true
            ))
        );
        assert_eq!(outcome(&view(State::Connecting)), None);
        assert!(!outcome(&view(State::Off)).expect("settled").1);
    }
}
