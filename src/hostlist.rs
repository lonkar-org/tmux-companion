//! The hosts picker, `hosts` in a terminal: each host's line, then its
//! sessions, with keys that act on the row and the list opened again over
//! what they did.
//!
//! It is one view of `hosts --print`, which says the same in plain lines:
//! the job "what is on my other machines, and do something about it" has
//! its screen-reader path there and in the commands each key runs (`hosts
//! connect`, `hosts edit`, `hosts forget`, `host kill`, `host rename`, `host
//! close`, `sessions start NAME@HOST`). What a
//! key did is said in words in the footer, and the keys a row takes are in
//! its preview, so nothing is said by colour or position alone.
//!
//! On a host's line: Enter brings its link up now, ctrl-e edits it, ctrl-x
//! forgets it, ctrl-o makes a session on it, ctrl-w closes every mirror of
//! it here. On a session: Enter opens its mirror here, ctrl-w closes that
//! mirror and leaves the session on the host, ctrl-x kills it there, ctrl-r
//! renames it there. ctrl-n adds a host from any row.

use crate::link::manager::HostView;
use crate::remote::{Row, Target};

/// The ctrl keys the picker takes, beside Enter and Esc. Not ctrl-d, which
/// cancels every picker, the way it does fzf.
pub const KEYS: [char; 6] = ['e', 'x', 'n', 'o', 'w', 'r'];

/// The footer while nothing has been said yet.
pub const FOOTER: &str = "enter connect or open   ctrl-w close here   ctrl-x forget or kill   \
                          ctrl-r rename   ctrl-o new session   ctrl-e edit   ctrl-n add   esc close";

/// The row the cursor was on, by what it stands for, so the list opened
/// again after an action starts there even when rows came or went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum At {
    /// A host's line.
    Host(String),
    /// One of a host's sessions.
    Session(String, String),
}

impl At {
    fn of(target: &Target) -> Self {
        match target {
            Target::Group(h) => At::Host(h.clone()),
            Target::Session { host, name, .. } => At::Session(host.clone(), name.clone()),
        }
    }

    fn host(&self) -> &str {
        match self {
            At::Host(h) | At::Session(h, _) => h,
        }
    }
}

/// Every host's line, then its sessions, the switcher's rows with nothing
/// left out: here a host's line is a row to act on, and a session with its
/// mirror open here is listed under its host with `open here` after it.
/// Each preview ends with the keys its row takes.
pub fn rows(views: &[HostView], open: &[(String, String)], now: u64) -> Vec<Row> {
    let mut rows = crate::remote::rows(views, &[], now);
    for r in &mut rows {
        match &r.target {
            Target::Group(h) => {
                let mirrors = open.iter().filter(|(oh, _)| oh == h).count();
                r.preview.push('\n');
                r.preview.push_str(&host_keys(h, mirrors));
            }
            Target::Session { host, name, .. } => {
                let here = open.iter().any(|(h, n)| h == host && n == name);
                if here {
                    r.line.push_str(", open here");
                }
                r.preview.push('\n');
                r.preview.push_str(&session_keys(host, name, here));
            }
        }
    }
    rows
}

/// The keys a host's line takes, one a line.
pub fn host_keys(host: &str, mirrors: usize) -> String {
    let mut out = format!(
        "enter brings {host}'s link up now\n\
         ctrl-e edits {host}: its command, and its name while nothing of it is open here\n\
         ctrl-x forgets {host}\n\
         ctrl-o makes a new session on {host}\n"
    );
    if mirrors > 0 {
        out.push_str(&format!(
            "ctrl-w closes the {} of {host} open here\n",
            if mirrors == 1 {
                "mirror".to_string()
            } else {
                format!("{mirrors} mirrors")
            }
        ));
    }
    out.push_str("ctrl-n adds a host\n");
    out
}

/// The keys a session's row takes, one a line.
pub fn session_keys(host: &str, name: &str, here: bool) -> String {
    let mut out = format!("enter opens {name}@{host} here\n");
    if here {
        out.push_str(&format!(
            "ctrl-w closes {name}@{host} here; {name} stays on {host}\n"
        ));
    }
    out.push_str(&format!(
        "ctrl-x kills {name} on {host}, or forgets {host}\n\
         ctrl-r renames {name} on {host}\n\
         ctrl-e edits {host}\n\
         ctrl-n adds a host\n"
    ));
    out
}

/// The rows as picker items: the switcher's look, every row a step, a
/// host's line included, since here it is something to act on.
fn items(rows: &[Row], views: &[HostView], now: u64) -> Vec<crate::picker::Item> {
    crate::remote::items(rows, views, now)
        .into_iter()
        .map(|mut i| {
            i.not_a_step = false;
            i
        })
        .collect()
}

/// The mirror sessions open on this tmux server, as `(host, session on the
/// host)`.
async fn open_here() -> Vec<(String, String)> {
    let format = format!(
        "#{{{}}}\t#{{{}}}",
        crate::link::HOST_OPTION,
        crate::link::REMOTE_OPTION
    );
    crate::cli::tmux_capture(&["list-sessions", "-F", &format])
        .await
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(h, r)| !h.is_empty() && !r.is_empty())
        .map(|(h, r)| (h.to_string(), r.to_string()))
        .collect()
}

/// What one key on one row ended with: a line to say and the list again,
/// or the picker done because the client went somewhere.
enum Then {
    Say(String),
    Done,
}

/// `hosts` in a terminal. With no hosts, the add form, and the list once one
/// is added.
pub async fn run() -> anyhow::Result<()> {
    let config = crate::cli::config_or_default();
    // The first ask brings each on-demand link up, so the list says where
    // each host is rather than what was cached; the asks after only read.
    let mut args = crate::proto::HostsArgs {
        connect: Some(String::new()),
        ..Default::default()
    };
    let mut said: Option<String> = None;
    let mut at: Option<At> = None;
    loop {
        let views = crate::hosts::ask(&args).await?;
        args.connect = None;
        if views.is_empty() {
            match crate::hosts::form().await? {
                Some(line) => {
                    said = Some(line);
                    continue;
                }
                None => return Ok(()),
            }
        }
        crate::hosts::paint_hosts(&views);
        let open = open_here().await;
        let now = crate::panes::now_secs();
        let rows = rows(&views, &open, now);
        let start = at
            .as_ref()
            .and_then(|a| rows.iter().position(|r| At::of(&r.target) == *a))
            .or_else(|| {
                // The row went, a session killed or renamed: its host's line.
                let host = at.as_ref()?.host().to_string();
                rows.iter()
                    .position(|r| At::of(&r.target) == At::Host(host.clone()))
            });
        let chrome = crate::picker::Chrome {
            title: "[ Hosts ]".into(),
            footer: said.take().unwrap_or_else(|| FOOTER.to_string()),
            prompt: "hosts> ".into(),
            icon: crate::tmux::icons::SSH.into(),
            ..Default::default()
        }
        .laid_out_like(&config.picker, crate::config::Picker::Project);
        let picked =
            crate::picker::run_keyed(items(&rows, &views, now), "", &chrome, &KEYS, start)?;
        let (i, key) = match picked {
            crate::picker::Keyed::Cancelled => return Ok(()),
            crate::picker::Keyed::Chosen(i) => (i, None),
            crate::picker::Keyed::Key(c, i) => (i, Some(c)),
        };
        let Some(row) = rows.get(i) else {
            return Ok(());
        };
        at = Some(At::of(&row.target));
        let here = |h: &str, n: &str| open.iter().any(|(oh, on)| oh == h && on == n);
        let then = match (&row.target, key) {
            (_, Some('n')) => Then::Say(
                crate::hosts::form()
                    .await?
                    .unwrap_or_else(|| "added no host".to_string()),
            ),
            (Target::Group(h), None) => Then::Say(connect(h).await),
            (Target::Group(h), Some('e')) | (Target::Session { host: h, .. }, Some('e')) => {
                Then::Say(edit(h).await?)
            }
            (Target::Group(h), Some('x')) => {
                if confirm(
                    h,
                    &format!("keep {h}"),
                    &format!("forget {h}: its sessions here and its entry in the config"),
                    &config,
                )? {
                    Then::Say(said_or_why(crate::hosts::forget(h).await))
                } else {
                    Then::Say(format!("kept {h}"))
                }
            }
            (Target::Group(h), Some('o')) => {
                crate::remote::new_session(h, None, &views).await?;
                Then::Done
            }
            (Target::Group(h), Some('w')) => Then::Say(close_all(h, &open).await),
            (Target::Group(h), Some('r')) => Then::Say(format!(
                "ctrl-r renames a session; ctrl-e edits {h}, its name among it"
            )),
            (
                Target::Session {
                    host,
                    name,
                    reachable,
                    ..
                },
                None,
            ) => {
                if *reachable || here(host, name) {
                    crate::remote::open(host, name, false, false).await?;
                    Then::Done
                } else {
                    Then::Say(connect(host).await)
                }
            }
            (Target::Session { host, name, .. }, Some('w')) => Then::Say(if here(host, name) {
                said_or_why(crate::remote::close_mirror(host, name).await)
            } else {
                format!("{name}@{host} isn't open here")
            }),
            (
                Target::Session {
                    host,
                    name,
                    reachable,
                },
                Some('x'),
            ) => Then::Say(row_act(host, name, *reachable, &config).await),
            (Target::Session { host, name, .. }, Some('r')) => {
                match ask_new_name(host, name, &config)? {
                    Some(new) => {
                        let line = said_or_why(crate::remote::rename(host, name, &new).await);
                        at = Some(At::Session(host.clone(), new));
                        Then::Say(line)
                    }
                    None => Then::Say(format!("{name} on {host} keeps its name")),
                }
            }
            (Target::Session { host, .. }, Some('o')) => {
                crate::remote::new_session(host, None, &views).await?;
                Then::Done
            }
            (_, Some(_)) => Then::Say(FOOTER.to_string()),
        };
        match then {
            Then::Say(line) => said = Some(line),
            Then::Done => return Ok(()),
        }
    }
}

/// The line on success, or why not.
fn said_or_why(r: anyhow::Result<String>) -> String {
    r.unwrap_or_else(|e| format!("{e:#}"))
}

/// Bring `host`'s link up now and say how it went.
async fn connect(host: &str) -> String {
    match crate::hosts::connect_lines(Some(host.to_string())).await {
        Ok(lines) => lines
            .into_iter()
            .map(|(l, _)| l)
            .collect::<Vec<_>>()
            .join("; "),
        Err(e) => format!("{e:#}"),
    }
}

/// The edit form for `host`, and what it kept.
async fn edit(host: &str) -> anyhow::Result<String> {
    let known = crate::cli::config_or_default()
        .hosts
        .into_iter()
        .chain(crate::link::manager::server_hosts().await)
        .find(|h| h.name == host);
    let Some(h) = known else {
        return Ok(format!("no host called {host}"));
    };
    Ok(crate::hosts::form_for(Some(&h))
        .await?
        .unwrap_or_else(|| format!("{host} is as it was")))
}

/// Ask what ctrl-x on `name@host` is for, the switcher's question, and do
/// it.
async fn row_act(
    host: &str,
    name: &str,
    reachable: bool,
    config: &crate::config::Config,
) -> String {
    use crate::remote::RowAct;
    match crate::remote::ask_row(host, name, reachable, config) {
        Ok(RowAct::Keep) => format!("kept {name}@{host}"),
        Ok(RowAct::Kill) => said_or_why(crate::remote::kill(host, name).await),
        Ok(RowAct::Drop) => {
            crate::remote::forget_row(host, name).await;
            format!("dropped {name}@{host}")
        }
        Ok(RowAct::Forget) => said_or_why(crate::hosts::forget(host).await),
        Err(e) => format!("{e:#}"),
    }
}

/// Close every mirror of `host` open here.
async fn close_all(host: &str, open: &[(String, String)]) -> String {
    let names: Vec<&String> = open
        .iter()
        .filter(|(h, _)| h == host)
        .map(|(_, n)| n)
        .collect();
    if names.is_empty() {
        return format!("nothing of {host} is open here");
    }
    let mut said = Vec::new();
    for n in names {
        said.push(said_or_why(crate::remote::close_mirror(host, n).await));
    }
    said.join("; ")
}

/// A two-row question, keep first so an Enter straight away does nothing.
/// True for the second row.
fn confirm(
    title: &str,
    keep: &str,
    act: &str,
    config: &crate::config::Config,
) -> anyhow::Result<bool> {
    let items = vec![
        crate::picker::Item::new(keep),
        crate::picker::Item::new(act),
    ];
    let chrome = crate::picker::Chrome {
        title: format!("[ {title} ]"),
        footer: "enter do it   esc keep".into(),
        icon: crate::tmux::icons::SSH.into(),
        ..Default::default()
    }
    .laid_out_like(&config.picker, crate::config::Picker::Project);
    Ok(crate::picker::run(items, "", &chrome)? == Some(1))
}

/// The words of the create row for a typed new name, or `None` for an empty
/// one or the name it has.
pub fn rename_row(typed: &str, host: &str, name: &str) -> Option<String> {
    let new = crate::sessions::start::tmux_name(typed.trim());
    (!new.is_empty() && new != name).then(|| format!("rename {name} on {host} to {new}"))
}

/// Ask for `name`'s new name on `host`, in a picker in this popup. `None`
/// for Esc or nothing typed.
fn ask_new_name(
    host: &str,
    name: &str,
    config: &crate::config::Config,
) -> anyhow::Result<Option<String>> {
    let chrome = crate::picker::Chrome {
        title: format!("[ Rename {name} on {host} ]"),
        footer: "type the new name   enter rename   esc keep the name".into(),
        prompt: "new name> ".into(),
        icon: crate::tmux::icons::SESSION.into(),
        create: Some({
            let (host, name) = (host.to_string(), name.to_string());
            crate::picker::Create::new(move |q| rename_row(q, &host, &name))
        }),
        ..Default::default()
    }
    .laid_out_like(&config.picker, crate::config::Picker::Project);
    Ok(
        match crate::picker::run_with_query(Vec::new(), "", &chrome)? {
            crate::picker::Outcome::Typed(q) => {
                let new = crate::sessions::start::tmux_name(q.trim());
                (!new.is_empty() && new != name).then_some(new)
            }
            _ => None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{
        Session,
        manager::{Kept, State},
    };

    fn view(name: &str, sessions: &[&str]) -> HostView {
        HostView {
            name: name.into(),
            command: vec!["ssh".into(), name.into()],
            link: crate::config::LinkMode::default(),
            kept: Kept::Config,
            state: State::Up,
            heard_at: 100,
            build: String::new(),
            tmux: String::new(),
            started_daemon: false,
            lacks: Vec::new(),
            attempts: 1,
            sessions: sessions
                .iter()
                .map(|s| Session {
                    name: (*s).into(),
                    windows: 2,
                    ..Session::default()
                })
                .collect(),
            hello_ms: 0,
            tried_at: 0,
            inbox: Vec::new(),
            open: crate::config::HostOpen::default(),
            pushing: true,
            poll_secs: 5,
        }
    }

    #[test]
    fn every_host_keeps_its_line_and_an_open_mirror_says_so() {
        let views = [view("vm", &["api", "web"]), view("box", &[])];
        let open = [("vm".to_string(), "web".to_string())];
        let rows = rows(&views, &open, 110);
        let lines: Vec<&str> = rows.iter().map(|r| r.line.as_str()).collect();
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines[0].starts_with("vm, 2 sessions"), "{lines:?}");
        assert!(lines[1].starts_with("api@vm"), "{lines:?}");
        assert!(
            lines[2].starts_with("web@vm") && lines[2].ends_with(", open here"),
            "{lines:?}"
        );
        assert!(lines[3].starts_with("box"), "{lines:?}");
        assert!(
            rows[0]
                .preview
                .contains("ctrl-w closes the mirror of vm open here")
        );
        assert!(
            !rows[3].preview.contains("ctrl-w"),
            "nothing of box is open"
        );
        assert!(
            rows[2]
                .preview
                .contains("ctrl-w closes web@vm here; web stays on vm")
        );
        assert!(!rows[1].preview.contains("ctrl-w"), "api isn't open here");
    }

    #[test]
    fn every_row_is_a_step_even_a_host_line() {
        let views = [view("vm", &["api"])];
        let rows = rows(&views, &[], 110);
        assert!(items(&rows, &views, 110).iter().all(|i| !i.not_a_step));
    }

    #[test]
    fn the_rename_row_needs_a_new_name() {
        assert_eq!(
            rename_row("web", "vm", "api"),
            Some("rename api on vm to web".into())
        );
        assert_eq!(rename_row(" api ", "vm", "api"), None);
        assert_eq!(rename_row("", "vm", "api"), None);
        assert_eq!(
            rename_row("a.b", "vm", "api"),
            Some("rename api on vm to a_b".into())
        );
    }
}
