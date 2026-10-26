//! Mirror sessions: the local session `api@vm` that stands for the session
//! `api` on a host, the grouped session `api~<this machine>` its pane attaches
//! to there, and the bar a mirror session shows.
//!
//! The bar is the Mac's, with the host's segments on the right: the daemon
//! asks the host's link for the grouped session's `segments` once a tick
//! while the mirror session has a client, renders them here from their
//! structured fields and sets the result as the mirror session's
//! `@tc-right`, in every bar mode. The host never sends markup: every string
//! it sends goes through [`super::clean`] and has its `#` doubled before it
//! reaches a tmux format, and a colour is used only when it reads as one, so
//! nothing a host says can reach `#(`.
//!
//! The window list is the host's too. A mirror session has one window of its
//! own, the pane the attach runs in, so its bar listed one window while
//! `M-a`, which goes to the host, listed them all. The host sends the grouped
//! session's windows with its segments, and while they are known the mirror
//! window's entry in the list is all of them, from `@tc-windows`; see
//! [`render_windows`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::Segment;
use crate::server::state::ServerState;

/// The mirror session's name for `remote` on `host`: `api@vm`. tmux turns
/// `.` and `:` in a session name into `_` itself, so this does too, and the
/// name it makes is the name it finds again. The session on the host is kept
/// whole in `@tc-remote`.
pub fn session_name(remote: &str, host: &str) -> String {
    format!("{}@{host}", crate::sessions::start::tmux_name(remote))
}

/// The grouped session's name on the host: `api~mac`, so two machines
/// attached to one host each get their own.
pub fn grouped_name(remote: &str, machine: &str) -> String {
    format!("{}~{machine}", crate::sessions::start::tmux_name(remote))
}

/// This machine's short name, for the grouped session's: the hostname up to
/// its first `.`, each character that isn't a letter, digit, `-` or `_`
/// turned into `-`, `mac` when there is none.
pub fn machine() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its whole length, and gethostname
    // writes at most that many bytes.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    let raw = if rc == 0 {
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    } else {
        String::new()
    };
    short_machine(&raw)
}

/// [`machine`] for a given hostname.
pub fn short_machine(hostname: &str) -> String {
    let short = hostname.split('.').next().unwrap_or("");
    let name: String = short
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(32)
        .collect();
    if name.is_empty() {
        "mac".to_string()
    } else {
        name
    }
}

/// The text of a host's string, made safe for a tmux format: cleaned, and
/// every `#` doubled so tmux prints it rather than expanding it.
pub fn literal(s: &str) -> String {
    super::clean(s).replace('#', "##")
}

/// A colour name a host sent, when it is one: `colourN`/`colorN` up to 255,
/// `#rrggbb`, `default`, or one of tmux's named colours. Anything else is
/// dropped and the bar's own colour used, since a colour goes inside
/// `#[...]` where a `,` or a `]` would be markup.
pub fn colour(name: &str) -> Option<String> {
    let n = name.trim();
    let named = [
        "default", "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ];
    let ok = named.contains(&n)
        || n.strip_prefix("bright")
            .is_some_and(|rest| named[1..].contains(&rest))
        || n.strip_prefix("colour")
            .or_else(|| n.strip_prefix("color"))
            .is_some_and(|d| {
                !d.is_empty() && d.len() <= 3 && d.parse::<u16>().is_ok_and(|v| v < 256)
            })
        || (n.len() == 7 && n.starts_with('#') && n[1..].chars().all(|c| c.is_ascii_hexdigit()));
    ok.then(|| n.to_string())
}

/// The glyph an `icons.rs` name stands for, nothing for a name that isn't
/// one: a host names a glyph, it never sends one.
pub fn glyph(name: &str) -> &'static str {
    crate::tmux::icons::by_name(name).unwrap_or("")
}

/// The bar's colours where a segment brings none.
const SEGMENT_FG: &str = "colour251";
const SEGMENT_BG: &str = "colour236";
/// The words about a link or this machine's health: quieter than a segment.
const NOTE_FG: &str = "colour245";
const BAR_BG: &str = "colour233";

/// The right side of a mirror session's bar: the host's segments, the host's
/// health said as `vm: <reason>`, then this machine's as `here: <reason>`.
/// With the link down it says `vm: no link` in words rather than leaving a
/// space, and shows no segment it heard before, since an old one would be
/// read as now.
pub fn render(host: &str, segments: Option<&[Segment]>, link_up: bool, here: &[String]) -> String {
    let host = literal(host);
    let mut out = String::new();
    match (link_up, segments) {
        (true, Some(segs)) => {
            let mut drew_runs = false;
            for s in segs {
                // The host's own drawing, from a host that sends it: its
                // colours where they read as colours, the bar's otherwise.
                if !s.runs.is_empty() {
                    for r in &s.runs {
                        let text = literal(&r.text);
                        if text.is_empty() {
                            continue;
                        }
                        let fg = colour(&r.fg).unwrap_or_else(|| "default".to_string());
                        let bg = colour(&r.bg).unwrap_or_else(|| BAR_BG.to_string());
                        out.push_str(&format!("#[fg={fg},bg={bg}]{text}"));
                    }
                    drew_runs = true;
                    continue;
                }
                if std::mem::take(&mut drew_runs) {
                    out.push(' ');
                }
                let text = literal(&s.text);
                if text.is_empty() {
                    continue;
                }
                let text = if s.role == "health" {
                    format!("{host}: {text}")
                } else {
                    text
                };
                let fg = colour(&s.fg).unwrap_or_else(|| SEGMENT_FG.to_string());
                let bg = colour(&s.bg).unwrap_or_else(|| SEGMENT_BG.to_string());
                out.push_str(&format!("#[fg={fg},bg={bg}] {}{text} ", glyph(&s.glyph)));
            }
            if drew_runs {
                out.push_str(&format!("#[bg={BAR_BG}] "));
            }
        }
        (true, None) => {}
        (false, _) => out.push_str(&format!("#[fg={NOTE_FG},bg={BAR_BG}] {host}: no link ")),
    }
    let here: Vec<String> = here
        .iter()
        .map(|r| literal(r))
        .filter(|r| !r.is_empty())
        .collect();
    if !here.is_empty() {
        out.push_str(&format!(
            "#[fg={NOTE_FG},bg={BAR_BG}] here: {} ",
            here.join("; ")
        ));
    }
    if !out.is_empty() {
        out.push_str("#[default]");
    }
    out
}

/// The session option holding the host's windows as the bar draws them.
pub const WINDOWS_OPTION: &str = "@tc-windows";

/// The session option keeping the mirror window's own `window-status-style`,
/// `window-status-current-style` and `window-status-separator`, tab
/// separated, while the host's windows stand in for it: a window entry's
/// `#[default]` is the window's own style, so the loop blanks both to give
/// each host tab a clean start, and puts them back from here, whichever
/// daemon set them.
pub const SAVED_OPTION: &str = "@tc-windows-style";

/// The host's windows as one window entry: each `index:name` with `*` on the
/// current one and `!` on one that rang, in the styles the window list
/// itself uses, `current` for the current tab and `normal` for the rest,
/// joined by the list's separator. Names go through [`literal`].
pub fn render_windows(windows: &[super::Window], normal: &str, current: &str, sep: &str) -> String {
    let style = |s: &str| {
        let s = s.trim();
        if s.is_empty() || s == "default" {
            String::new()
        } else {
            format!("#[{s}]")
        }
    };
    windows
        .iter()
        .map(|w| {
            let mut flags = String::new();
            if w.active {
                flags.push('*');
            }
            if w.bell {
                flags.push('!');
            }
            format!(
                "#[default]{}{}:{}{flags}#[default]",
                style(if w.active { current } else { normal }),
                w.index,
                literal(&w.name)
            )
        })
        .collect::<Vec<_>>()
        .join(sep)
}

/// The window list's styles and separator for one mirror window, as saved
/// in [`SAVED_OPTION`] or read off the window before it was blanked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListStyle {
    /// `window-status-style`.
    pub normal: String,
    /// `window-status-current-style`.
    pub current: String,
    /// `window-status-separator`.
    pub sep: String,
}

impl ListStyle {
    fn saved(&self) -> String {
        format!("{}\t{}\t{}", self.normal, self.current, self.sep)
    }

    /// Read from the one line [`STYLE_FORMAT`] prints: the saved copy when
    /// there is one, with true, since the window is already showing the
    /// host's; the window's own values otherwise, with false.
    fn read(line: &str) -> (ListStyle, bool) {
        // The saved copy is itself three fields, or nothing: six fields with
        // one, four without. Telling them apart by whether the first fields
        // were empty read a window's own `default` style as a saved
        // separator, and the bar said `1:aidefault2:zsh*`.
        let f: Vec<&str> = line.split('\t').collect();
        let at = |i: usize| ListStyle {
            normal: f.get(i).copied().unwrap_or("").to_string(),
            current: f.get(i + 1).copied().unwrap_or("").to_string(),
            sep: f.get(i + 2).copied().unwrap_or("").to_string(),
        };
        if f.len() >= 6 {
            (at(0), true)
        } else {
            (at(1), false)
        }
    }
}

/// What the loop reads off a mirror window before it first draws the host's
/// windows there: the saved copy, three fields or none, then the window's
/// own three.
const STYLE_FORMAT: &str = "#{@tc-windows-style}\t#{E:window-status-style}\t#{E:window-status-current-style}\t#{window-status-separator}";

/// The tmux commands that put the host's windows in a mirror window's entry,
/// `value` from [`render_windows`]: the window's own styles kept in
/// [`SAVED_OPTION`] and blanked, both formats reading [`WINDOWS_OPTION`].
pub fn show_commands(session: &str, value: &str, style: &ListStyle) -> Vec<Vec<String>> {
    let t = format!("={session}:");
    let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let reads = format!("#{{{WINDOWS_OPTION}}}");
    vec![
        s(&["set-option", "-t", &t, SAVED_OPTION, &style.saved()]),
        s(&["set-option", "-t", &t, WINDOWS_OPTION, value]),
        s(&[
            "set-option",
            "-w",
            "-t",
            &t,
            "window-status-style",
            "default",
        ]),
        s(&[
            "set-option",
            "-w",
            "-t",
            &t,
            "window-status-current-style",
            "default",
        ]),
        s(&["set-option", "-w", "-t", &t, "window-status-format", &reads]),
        s(&[
            "set-option",
            "-w",
            "-t",
            &t,
            "window-status-current-format",
            &reads,
        ]),
    ]
}

/// The tmux commands that give a mirror window its own entry back: its
/// styles from `style`, the formats the global ones again, and both options
/// gone.
pub fn release_commands(session: &str, style: &ListStyle) -> Vec<Vec<String>> {
    let t = format!("={session}:");
    let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let or_default = |v: &str| {
        if v.is_empty() {
            "default".to_string()
        } else {
            v.to_string()
        }
    };
    vec![
        s(&[
            "set-option",
            "-w",
            "-t",
            &t,
            "window-status-style",
            &or_default(&style.normal),
        ]),
        s(&[
            "set-option",
            "-w",
            "-t",
            &t,
            "window-status-current-style",
            &or_default(&style.current),
        ]),
        s(&["set-option", "-wu", "-t", &t, "window-status-format"]),
        s(&[
            "set-option",
            "-wu",
            "-t",
            &t,
            "window-status-current-format",
        ]),
        s(&["set-option", "-u", "-t", &t, WINDOWS_OPTION]),
        s(&["set-option", "-u", "-t", &t, SAVED_OPTION]),
    ]
}

/// One tmux call for many commands, `a ; b ; c`.
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

/// A mirror window's list style, read in one call.
async fn read_style(session: &str) -> Option<(ListStyle, bool)> {
    let out = crate::tmux::command()
        .args([
            "display-message",
            "-p",
            "-t",
            &format!("={session}:"),
            STYLE_FORMAT,
        ])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(ListStyle::read(
        String::from_utf8_lossy(&out.stdout).trim_end_matches('\n'),
    ))
}

/// Draw or release the host's windows in each of `changed`'s mirror windows:
/// `Some` draws them, `None` gives the window its own entry back. A window
/// whose styles can't be read is left for the next tick.
async fn write_windows(
    changed: &[(String, Option<Vec<super::Window>>)],
    styles: &mut HashMap<String, ListStyle>,
) -> Vec<String> {
    let mut cmds = Vec::new();
    let mut done = Vec::new();
    for (session, wins) in changed {
        let style = match styles.get(session) {
            Some(st) => Some((st.clone(), true)),
            None => read_style(session).await,
        };
        let Some((style, taken)) = style else {
            continue;
        };
        match wins {
            Some(w) => {
                let value = render_windows(w, &style.normal, &style.current, &style.sep);
                cmds.extend(show_commands(session, &value, &style));
                styles.insert(session.clone(), style);
            }
            None if taken => {
                cmds.extend(release_commands(session, &style));
                styles.remove(session);
            }
            None => {}
        }
        done.push(session.clone());
    }
    if !cmds.is_empty() {
        let ok = crate::tmux::command()
            .args(chained(cmds))
            .output()
            .await
            .is_ok_and(|o| o.status.success());
        if !ok {
            return Vec::new();
        }
    }
    done
}

/// The tmux commands that paint the mirror session `session` with its host
/// session's theme: this machine's `_reset.tmux`, the host's settings as
/// `set-option` arguments, this machine's `_apply.tmux`, then `@theme-file`.
///
/// Arguments and never a sourced file, because the settings came from
/// another machine: [`super::read_theme`] has already checked each one, and
/// an argument can't become a second command whatever it holds. The two `_`
/// files are this machine's own, so what a theme changes here is still what
/// this machine's `_apply.tmux` says.
pub fn theme_commands(
    session: &str,
    theme: &super::Theme,
    dir: &std::path::Path,
) -> Vec<Vec<String>> {
    let t = format!("={session}:");
    let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let reset = dir.join("_reset.tmux").display().to_string();
    let apply = dir.join("_apply.tmux").display().to_string();
    let mut cmds = vec![s(&["source-file", "-t", &t, &reset])];
    for (k, v) in &theme.settings {
        cmds.push(s(&["set-option", "-t", &t, k, v]));
    }
    cmds.push(s(&["source-file", "-t", &t, &apply]));
    cmds.push(if theme.file.is_empty() {
        s(&["set-option", "-u", "-t", &t, "@theme-file"])
    } else {
        s(&["set-option", "-t", &t, "@theme-file", &theme.file])
    });
    cmds
}

/// Paint each of `changed`'s mirror sessions with its host session's theme,
/// returning the ones painted. Nothing when this machine has no
/// `_apply.tmux`: without `theme init` here there is nothing to paint with,
/// and the session keeps whatever it had.
///
/// A theme this machine doesn't have is written to the themes directory
/// too, so the picker here lists it; a file already there by that name is
/// left alone, it may be somebody's own.
async fn paint(
    changed: &[(String, super::Theme)],
    host_of: &HashMap<String, String>,
) -> Vec<String> {
    let dir = crate::theme::default_themes_dir();
    if !dir.join("_apply.tmux").is_file() || !dir.join("_reset.tmux").is_file() {
        return Vec::new();
    }
    let mut cmds = Vec::new();
    for (session, theme) in changed {
        cmds.extend(theme_commands(session, theme, &dir));
    }
    let ok = crate::tmux::command()
        .args(chained(cmds))
        .output()
        .await
        .is_ok_and(|o| o.status.success());
    if !ok {
        return Vec::new();
    }
    for (session, theme) in changed {
        if theme.file.is_empty() {
            continue;
        }
        let path = dir.join(format!("{}.tmux", theme.file));
        if !path.exists() {
            let host = host_of.get(session).map(String::as_str).unwrap_or("");
            let text = crate::theme::mirrored_theme_file(host, theme, &dir.display().to_string());
            let _ = std::fs::write(&path, text);
        }
    }
    changed.iter().map(|(s, _)| s.clone()).collect()
}

/// Say `theme Amber on api@vm` on the message line of each client in a
/// mirror session whose colour changed while somebody was in it.
async fn tell(said: &[(String, String)]) {
    let listing = crate::tmux::command()
        .args(["list-clients", "-F", "#{client_name}\t#{client_session}"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut cmds = Vec::new();
    for (session, message) in said {
        for client in crate::theme::clients_to_tell(&listing, std::slice::from_ref(session)) {
            cmds.push(vec![
                "display-message".to_string(),
                "-c".to_string(),
                client,
                message.clone(),
            ]);
        }
    }
    if !cmds.is_empty() {
        let _ = crate::tmux::command().args(chained(cmds)).status().await;
    }
}

/// Whether a mirror session needs painting with its host's `theme`: never
/// painted, painted with another, or repainted since by something here. A
/// `theme apply --all` after a reload paints every session from this
/// machine's map, a mirror session included, and the pane poll's
/// `@theme-file` is how the loop sees it. Trusted only once the paint is
/// older than any poll the loop takes, or a poll from before it would read
/// as the paint undone and repaint every tick until the next one.
pub fn needs_paint(
    painted: Option<&(super::Theme, Instant)>,
    theme: &super::Theme,
    session: &str,
    panes: &[crate::panes::Pane],
) -> bool {
    match painted {
        Some((t, _)) if t != theme => true,
        None => true,
        Some((_, at)) => {
            at.elapsed() > LOOK
                && panes
                    .iter()
                    .find(|p| p.session == session)
                    .is_some_and(|p| p.theme_file != theme.file)
        }
    }
}

/// The `rename-session`s that rename `session` to `new`, in the order they
/// have to run: every mirror's grouped session to `new~machine` first, then
/// the session itself. Run inside a mirror's grouped session, `api~mac`,
/// the session renamed is its group's own, `api`: what a rename there
/// means. `names` is every session on the server, for the names a rename
/// would take. Empty when there is nothing to do.
pub fn rename_plan(
    members: &[super::Member],
    names: &[String],
    session: &str,
    new: &str,
) -> Result<Vec<(String, String)>, String> {
    let new = crate::sessions::start::tmux_name(new.trim());
    if new.is_empty() {
        return Err("a session needs a name".to_string());
    }
    let me = members.iter().find(|m| m.name == session);
    let base = match me {
        Some(m) if !m.mirror.is_empty() => {
            let own: Vec<&super::Member> = members
                .iter()
                .filter(|o| o.group == m.group && o.mirror.is_empty())
                .collect();
            match own.as_slice() {
                [one] => one.name.clone(),
                [] => return Err(format!("{session} is a mirror's, and its session is gone")),
                _ => {
                    return Err(format!(
                        "{session} is in a group with more than one session of its own; rename one of them"
                    ));
                }
            }
        }
        _ => session.to_string(),
    };
    let group = members
        .iter()
        .find(|m| m.name == base)
        .map(|m| m.group.clone());
    let mut plan: Vec<(String, String)> = members
        .iter()
        .filter(|m| !m.mirror.is_empty() && Some(&m.group) == group.as_ref())
        .map(|m| (m.name.clone(), grouped_name(&new, &m.mirror)))
        .filter(|(from, to)| from != to)
        .collect();
    if base != new {
        plan.push((base, new));
    }
    let freed: Vec<&String> = plan.iter().map(|(f, _)| f).collect();
    if let Some((_, to)) = plan
        .iter()
        .find(|(_, to)| names.contains(to) && !freed.contains(&to))
    {
        return Err(format!("{to} is taken"));
    }
    Ok(plan)
}

/// One step of [`follow_plan`]: the renames, in order, and the project-map
/// key they move from one name to another.
pub type FollowStep = (Vec<(String, String)>, Option<(String, String)>);

/// What the host has to do to keep each mirror's grouped session named
/// `session~machine` after a rename made some other way, each step with the
/// project-map key it moves. A grouped session still named for the session's
/// old name, `api~mac` beside `web`, follows it. One renamed to a plain name
/// is somebody pressing `prefix $` inside a mirror, which renames the
/// grouped session: that name is meant for the session, and it is renamed
/// there, with every mirror following.
pub fn follow_plan(members: &[super::Member], names: &[String]) -> Vec<FollowStep> {
    let mut steps = Vec::new();
    let mut groups: Vec<&str> = members.iter().map(|m| m.group.as_str()).collect();
    groups.sort_unstable();
    groups.dedup();
    for g in groups {
        let own: Vec<&super::Member> = members
            .iter()
            .filter(|m| m.group == g && m.mirror.is_empty())
            .collect();
        let [base] = own.as_slice() else {
            continue;
        };
        let mut renames = Vec::new();
        let mut moved = None;
        for m in members
            .iter()
            .filter(|m| m.group == g && !m.mirror.is_empty())
        {
            let want = grouped_name(&base.name, &m.mirror);
            if m.name == want {
                continue;
            }
            match m.name.strip_suffix(&format!("~{}", m.mirror)) {
                Some(old) => {
                    moved.get_or_insert((old.to_string(), base.name.clone()));
                    renames.push((m.name.clone(), want));
                }
                None => {
                    if let Ok(plan) = rename_plan(members, names, &base.name, &m.name) {
                        steps.push((plan, Some((base.name.clone(), m.name.clone()))));
                    }
                    renames.clear();
                    moved = None;
                    break;
                }
            }
        }
        if !renames.is_empty() {
            steps.push((renames, moved));
        }
    }
    steps
}

/// Run a rename plan and move its project-map key, saying `api is now web`
/// on the message line of each client on the renamed session.
pub async fn apply_rename(plan: &[(String, String)], moved: Option<&(String, String)>) -> bool {
    if plan.is_empty() {
        return true;
    }
    let cmds: Vec<Vec<String>> = plan
        .iter()
        .map(|(from, to)| {
            vec![
                "rename-session".to_string(),
                "-t".to_string(),
                format!("={from}"),
                to.clone(),
            ]
        })
        .collect();
    let ok = crate::tmux::command()
        .args(chained(cmds))
        .output()
        .await
        .is_ok_and(|o| o.status.success());
    if !ok {
        return false;
    }
    if let Some((old, new)) = moved {
        let map = crate::theme::default_themes_dir().join("_project-map.tsv");
        if let Ok(text) = std::fs::read_to_string(&map)
            && let Some(moved) = crate::theme::with_map_moved(&text, old, new)
        {
            let _ = std::fs::write(&map, moved);
        }
        let _ = crate::tmux::command()
            .args([
                "display-message",
                "-t",
                &format!("={new}:"),
                &crate::picker::literal_for_tmux(&format!("{old} is now {new}")),
            ])
            .output()
            .await;
    }
    true
}

/// `sessions rename NEW`: [`rename_plan`] for the session this runs in, or
/// the one named, then run. A mirror session here stands for a session on
/// its host, and is renamed there, by `prefix $` inside it or this command
/// on the host; this machine's mirror follows.
pub async fn rename_command(session: Option<String>, new: &str) -> anyhow::Result<String> {
    let target = session
        .or_else(crate::theme::cli::calling_session)
        .ok_or_else(|| anyhow::anyhow!("not inside tmux; say which session with --session"))?;
    let session = crate::theme::cli::session_of(&format!("={target}:"))
        .or_else(|| crate::theme::cli::session_of(&target))
        .ok_or_else(|| anyhow::anyhow!("no session {target}"))?;
    let out = crate::tmux::command()
        .args([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_group}\t#{@tc-mirror}\t#{@tc-host}\t#{@tc-remote}",
        ])
        .output()
        .await?;
    let listing = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut members = Vec::new();
    let mut names = Vec::new();
    for l in listing.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        let [name, group, mirror, host, remote] = f[..] else {
            continue;
        };
        if name == session && !host.is_empty() {
            anyhow::bail!(
                "{session} stands for {remote} on {host}: rename it there, where prefix $ inside it already does, and this mirror follows"
            );
        }
        names.push(name.to_string());
        if !group.is_empty() {
            members.push(super::Member {
                name: name.to_string(),
                group: group.to_string(),
                mirror: mirror.to_string(),
            });
        }
    }
    let plan = rename_plan(&members, &names, &session, new).map_err(anyhow::Error::msg)?;
    let Some(moved) = plan
        .iter()
        .find(|(from, _)| {
            !members
                .iter()
                .any(|m| &m.name == from && !m.mirror.is_empty())
        })
        .cloned()
    else {
        return Ok(format!("{session} is already called that"));
    };
    if !apply_rename(&plan, Some(&moved)).await {
        anyhow::bail!("tmux refused the rename of {}", moved.0);
    }
    Ok(format!("{} is now {}", moved.0, moved.1))
}

/// The host's half of following a rename: the steps [`follow_plan`] finds
/// in the listing the daemon last read, run, and the listing read again so
/// the next tick doesn't find them twice. Starts no process while every
/// grouped session is named as it should be.
async fn follow_renames(state: &Arc<Mutex<ServerState>>) {
    let (members, names) = {
        let st = state.lock().await;
        if st.members.iter().all(|m| m.mirror.is_empty()) {
            return;
        }
        let names: Vec<String> = st
            .session_list
            .as_ref()
            .map(|(s, _, _)| s.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default();
        (st.members.clone(), names)
    };
    let mut names = names;
    names.extend(members.iter().map(|m| m.name.clone()));
    let steps = follow_plan(&members, &names);
    if steps.is_empty() {
        return;
    }
    for (plan, moved) in &steps {
        apply_rename(plan, moved.as_ref()).await;
    }
    let _ = crate::panes::shared(state, Duration::ZERO).await;
}

/// The host session's new name when the session `mirror` stands for was
/// renamed there: tmux keeps a session's `$N` through a rename, so the id
/// learned while the name still matched finds it under the new one. Learns
/// the id into `ids` as it goes. `None` while the name matches, and while
/// the host's list hasn't been heard.
pub fn renamed_remote(
    mirror: &super::Mirror,
    sessions: &[super::Session],
    ids: &mut HashMap<String, String>,
) -> Option<String> {
    if let Some(s) = sessions.iter().find(|s| s.name == mirror.remote) {
        ids.insert(mirror.session.clone(), s.id.clone());
        return None;
    }
    let id = ids.get(&mirror.session)?;
    sessions
        .iter()
        .find(|s| &s.id == id && !s.id.is_empty())
        .map(|s| s.name.clone())
}

/// This machine's half of following a rename: each mirror session whose
/// host session now has another name takes it, `api@vm` to `web@vm` with
/// `@tc-remote` `web`, and says `api on vm is now web` to its clients.
/// True when something was renamed, so the loop reads the listing again
/// before anything else uses the old name.
async fn follow_hosts(
    state: &Arc<Mutex<ServerState>>,
    mirrors: &[super::Mirror],
    ids: &mut HashMap<String, String>,
) -> bool {
    let found: Vec<(super::Mirror, String)> = {
        let st = state.lock().await;
        mirrors
            .iter()
            .filter_map(|m| {
                let h = st.links.hosts.iter().find(|h| h.host.name == m.host)?;
                if h.sessions.is_empty() {
                    return None;
                }
                renamed_remote(m, &h.sessions, ids).map(|new| (m.clone(), new))
            })
            .collect()
    };
    let mut any = false;
    for (m, new) in found {
        let to = session_name(&new, &m.host);
        let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let said =
            crate::picker::literal_for_tmux(&format!("{} on {} is now {new}", m.remote, m.host));
        let cmds = vec![
            s(&["rename-session", "-t", &format!("={}", m.session), &to]),
            s(&[
                "set-option",
                "-t",
                &format!("={to}:"),
                super::REMOTE_OPTION,
                &new,
            ]),
            s(&["display-message", "-t", &format!("={to}:"), &said]),
        ];
        let ok = crate::tmux::command()
            .args(chained(cmds))
            .output()
            .await
            .is_ok_and(|o| o.status.success());
        if ok {
            if let Some(id) = ids.remove(&m.session) {
                ids.insert(to, id);
            }
            any = true;
        }
    }
    any
}

/// How old a session listing the loop takes: mirror sessions open and close
/// far less often than a bar changes, so the listing can be the one another
/// loop (the inbox's every two seconds, the push loop's every one) already
/// read. A mirror session just opened gets its bar within this.
const LOOK: Duration = Duration::from_secs(5);

/// The daemon's mirror bar: once a tick, for each mirror session with a
/// client, the host's segments rendered and set as its `@tc-right` when they
/// changed. It reads the shared pane poll, which carries the session list,
/// up to `LOOK` old, so it starts no process of its own when another loop
/// has read one; with no host configured it reads nothing at all. With no
/// mirror open it looks once every `LOOK` rather than every tick.
pub async fn mirror_loop(state: Arc<Mutex<ServerState>>) {
    let machine = machine();
    let mut tick = crate::push::ticker();
    let mut last: HashMap<String, String> = HashMap::new();
    // The windows last drawn in each mirror window, and the styles they
    // were drawn in.
    let mut drawn: HashMap<String, Option<Vec<super::Window>>> = HashMap::new();
    let mut styles: HashMap<String, ListStyle> = HashMap::new();
    // The theme each mirror session was last painted with, and when. Kept
    // while the session is open, so a client coming back doesn't repaint it
    // or hear about a colour it already had.
    let mut painted: HashMap<String, (super::Theme, Instant)> = HashMap::new();
    // Each mirror session's host session id, learned while its name
    // matched, to find it again after a rename on the host.
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut looked: Option<Instant> = None;
    let mut any = false;
    loop {
        tick.tick().await;
        follow_renames(&state).await;
        if state.lock().await.links.hosts.is_empty() {
            last.clear();
            continue;
        }
        if !any && looked.is_some_and(|t| t.elapsed() < LOOK) {
            continue;
        }
        looked = Some(Instant::now());
        let panes = crate::panes::shared(&state, LOOK).await;
        let mirrors = state.lock().await.mirrors.clone();
        any = !mirrors.is_empty();
        ids.retain(|s, _| mirrors.iter().any(|m| &m.session == s));
        if follow_hosts(&state, &mirrors, &mut ids).await {
            let _ = crate::panes::shared(&state, Duration::ZERO).await;
            continue;
        }
        let open: Vec<String> = mirrors.iter().map(|m| m.host.clone()).collect();
        let shown: Vec<&super::Mirror> = mirrors.iter().filter(|m| m.attached > 0).collect();
        let wants: Vec<(String, String)> = shown
            .iter()
            .map(|m| (m.host.clone(), grouped_name(&m.remote, &machine)))
            .collect();
        state.lock().await.links.set_mirrors(&open, &wants);
        last.retain(|s, _| shown.iter().any(|m| &m.session == s));
        drawn.retain(|s, _| shown.iter().any(|m| &m.session == s));
        styles.retain(|s, _| mirrors.iter().any(|m| &m.session == s));
        painted.retain(|s, _| mirrors.iter().any(|m| &m.session == s));
        if shown.is_empty() {
            continue;
        }
        let here = crate::server::health_reasons(&state).await;
        let mut changed = Vec::new();
        let mut windows_changed = Vec::new();
        let mut themes_changed = Vec::new();
        for (m, (_, grouped)) in shown.iter().zip(&wants) {
            if let Some(theme) = state.lock().await.links.theme_for(&m.host, grouped)
                && needs_paint(painted.get(&m.session), &theme, &m.session, &panes)
            {
                themes_changed.push((m.session.clone(), theme));
            }
            let wins = state.lock().await.links.windows_for(&m.host, grouped);
            if drawn.get(&m.session) != Some(&wins) {
                windows_changed.push((m.session.clone(), wins));
            }
            let (segs, up) = state.lock().await.links.segments_for(&m.host, grouped);
            let value = render(&m.host, segs.as_ref().map(|(s, _)| s.as_slice()), up, &here);
            if last.get(&m.session) != Some(&value) {
                last.insert(m.session.clone(), value.clone());
                changed.push((m.session.clone(), value));
            }
        }
        if !windows_changed.is_empty() {
            for s in write_windows(&windows_changed, &mut styles).await {
                if let Some((_, w)) = windows_changed.iter().find(|(x, _)| *x == s) {
                    drawn.insert(s, w.clone());
                }
            }
        }
        if !themes_changed.is_empty() {
            let host_of: HashMap<String, String> = shown
                .iter()
                .map(|m| (m.session.clone(), m.host.clone()))
                .collect();
            let mut said = Vec::new();
            for s in paint(&themes_changed, &host_of).await {
                if let Some((_, theme)) = themes_changed.iter().find(|(x, _)| *x == s) {
                    // Said only for a change of theme: opening a mirror
                    // paints it without a word, as opening a session here
                    // does, and so does putting back one a reload undid.
                    if painted.get(&s).is_some_and(|(t, _)| t != theme) {
                        said.push((s.clone(), format!("theme {} on {s}", theme.name())));
                    }
                    painted.insert(s, (theme.clone(), Instant::now()));
                }
            }
            tell(&said).await;
        }
        if changed.is_empty() {
            continue;
        }
        if !crate::push::write_sessions(&changed).await {
            for (s, _) in &changed {
                last.remove(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, group: &str, mirror: &str) -> super::super::Member {
        super::super::Member {
            name: name.into(),
            group: group.into(),
            mirror: mirror.into(),
        }
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn a_rename_inside_a_mirror_renames_its_session_and_every_mirror_follows() {
        let members = [
            member("api", "api", ""),
            member("api~mac", "api", "mac"),
            member("api~m1", "api", "m1"),
        ];
        let names: Vec<String> = ["api", "api~mac", "api~m1", "other"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            rename_plan(&members, &names, "api~mac", "web"),
            Ok(pairs(&[
                ("api~mac", "web~mac"),
                ("api~m1", "web~m1"),
                ("api", "web")
            ]))
        );
        assert_eq!(
            rename_plan(&members, &names, "api", "web"),
            rename_plan(&members, &names, "api~mac", "web")
        );
        assert_eq!(
            rename_plan(&members, &names, "api", "other"),
            Err("other is taken".to_string())
        );
        assert_eq!(
            rename_plan(&members, &names, "other", "x.y"),
            Ok(pairs(&[("other", "x_y")]))
        );
        assert!(rename_plan(&members, &names, "api", " ").is_err());
    }

    #[test]
    fn a_mirror_finds_its_session_again_by_id_after_a_rename() {
        let m = super::super::Mirror {
            session: "api@vm".into(),
            host: "vm".into(),
            remote: "api".into(),
            attached: 1,
        };
        let session = |name: &str, id: &str| super::super::Session {
            name: name.into(),
            id: id.into(),
            ..Default::default()
        };
        let mut ids = HashMap::new();
        // Never heard the name: nothing learned, nothing to follow.
        assert_eq!(renamed_remote(&m, &[session("web", "$3")], &mut ids), None);
        assert!(ids.is_empty());
        assert_eq!(renamed_remote(&m, &[session("api", "$3")], &mut ids), None);
        assert_eq!(ids.get("api@vm").map(String::as_str), Some("$3"));
        assert_eq!(
            renamed_remote(&m, &[session("web", "$3"), session("x", "$4")], &mut ids).as_deref(),
            Some("web")
        );
        // Gone altogether: nothing to follow.
        assert_eq!(renamed_remote(&m, &[session("x", "$4")], &mut ids), None);
    }

    #[test]
    fn the_host_follows_a_rename_made_any_other_way() {
        let names =
            |m: &[super::super::Member]| m.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
        // `api` renamed `web` on the host: its mirrors follow it.
        let moved = [member("web", "api", ""), member("api~mac", "api", "mac")];
        assert_eq!(
            follow_plan(&moved, &names(&moved)),
            [(
                pairs(&[("api~mac", "web~mac")]),
                Some(("api".to_string(), "web".to_string()))
            )]
        );
        // `prefix $` inside the mirror renamed `api~mac` to `web`: that is
        // the session's new name.
        let typed = [
            member("api", "api", ""),
            member("web", "api", "mac"),
            member("api~m1", "api", "m1"),
        ];
        assert_eq!(
            follow_plan(&typed, &names(&typed)),
            [(
                pairs(&[("web", "web~mac"), ("api~m1", "web~m1"), ("api", "web")]),
                Some(("api".to_string(), "web".to_string()))
            )]
        );
        // Named as they should be: nothing.
        let fine = [member("api", "api", ""), member("api~mac", "api", "mac")];
        assert!(follow_plan(&fine, &names(&fine)).is_empty());
        // A group of the host's own with two sessions and no mirror: not ours.
        let own = [member("a", "a", ""), member("b", "a", "")];
        assert!(follow_plan(&own, &names(&own)).is_empty());
    }

    fn amber() -> super::super::Theme {
        super::super::Theme {
            file: "amber".into(),
            settings: vec![
                ("@theme-color-main-1".into(), "colour172".into()),
                ("@theme-name".into(), "Amber".into()),
            ],
        }
    }

    #[test]
    fn a_host_theme_is_set_as_arguments_between_this_machines_files() {
        let cmds = theme_commands("api@vm", &amber(), std::path::Path::new("/t"));
        let want: Vec<Vec<&str>> = vec![
            vec!["source-file", "-t", "=api@vm:", "/t/_reset.tmux"],
            vec![
                "set-option",
                "-t",
                "=api@vm:",
                "@theme-color-main-1",
                "colour172",
            ],
            vec!["set-option", "-t", "=api@vm:", "@theme-name", "Amber"],
            vec!["source-file", "-t", "=api@vm:", "/t/_apply.tmux"],
            vec!["set-option", "-t", "=api@vm:", "@theme-file", "amber"],
        ];
        assert_eq!(cmds, want);
    }

    #[test]
    fn a_mirror_is_painted_when_new_changed_or_undone_here() {
        let pane = |file: &str| {
            crate::panes::parse(&format!(
                "api@vm\t1\tzsh\t0\t%1\tzsh\t/\t\t1\t1\t1\t0\t0\th\t0\t$1\t\t1\t\t{file}"
            ))
        };
        let t = amber();
        let long_ago = Instant::now() - LOOK - Duration::from_secs(1);
        assert!(needs_paint(None, &t, "api@vm", &pane("amber")));
        let mut other = t.clone();
        other.file = "ink".into();
        assert!(needs_paint(
            Some(&(other, long_ago)),
            &t,
            "api@vm",
            &pane("amber")
        ));
        assert!(!needs_paint(
            Some(&(t.clone(), long_ago)),
            &t,
            "api@vm",
            &pane("amber")
        ));
        // A reload painted it from this machine's map.
        assert!(needs_paint(
            Some(&(t.clone(), long_ago)),
            &t,
            "api@vm",
            &pane("pine")
        ));
        // The same, read off a poll that may be older than the paint.
        assert!(!needs_paint(
            Some(&(t.clone(), Instant::now())),
            &t,
            "api@vm",
            &pane("pine")
        ));
    }

    #[test]
    fn a_theme_written_for_a_mirror_reads_back_the_same() {
        let text = crate::theme::mirrored_theme_file("vm", &amber(), "/t");
        assert!(text.contains("source-file \"/t/_reset.tmux\""), "{text}");
        assert!(text.contains("source-file \"/t/_apply.tmux\""), "{text}");
        let back = crate::theme::parse_settings(&text);
        for (k, v) in amber().settings {
            assert_eq!(back.get(&k), Some(&v), "{text}");
        }
    }

    fn seg(role: &str, text: &str, fg: &str, bg: &str, glyph: &str) -> Segment {
        Segment {
            role: role.into(),
            text: text.into(),
            fg: fg.into(),
            bg: bg.into(),
            glyph: glyph.into(),
            ..Segment::default()
        }
    }

    fn run(text: &str, fg: &str, bg: &str) -> crate::link::Run {
        crate::link::Run {
            text: text.into(),
            fg: fg.into(),
            bg: bg.into(),
        }
    }

    #[test]
    fn a_hosts_runs_are_drawn_as_it_drew_them_in_its_order() {
        let chunk = Segment {
            role: "chunk".into(),
            text: "󱫐 37m".into(),
            runs: vec![run("󱫐 37m", "colour114", "color233")],
            ..Segment::default()
        };
        let agents = Segment {
            runs: vec![run(" 󰚩 2", "", "color233"), run(" 󰦖 2", "colour33", "")],
            ..seg("agents", "2 agents, 2 busy", "", "", "AGENT")
        };
        let out = render(
            "vm",
            Some(&[chunk, agents, seg("health", "offline", "", "", "")]),
            true,
            &[],
        );
        assert_eq!(
            out,
            "#[fg=colour114,bg=color233]󱫐 37m\
             #[fg=default,bg=color233] 󰚩 2\
             #[fg=colour33,bg=colour233] 󰦖 2 \
             #[fg=colour251,bg=colour236] vm: offline \
             #[default]"
        );
        // Words alone, from a host from before runs, are drawn as they were.
        let old = render(
            "vm",
            Some(&[seg("agents", "1 agent", "", "", "AGENT")]),
            true,
            &[],
        );
        assert_eq!(old, "#[fg=colour251,bg=colour236] 󰚩 1 agent #[default]");
    }

    #[test]
    fn names_follow_tmux_and_the_machine() {
        assert_eq!(session_name("api", "vm"), "api@vm");
        assert_eq!(session_name("a.b:c", "vm"), "a_b_c@vm");
        assert_eq!(grouped_name("api", "mac"), "api~mac");
        assert_eq!(short_machine("Yogeshs-MacBook.local"), "Yogeshs-MacBook");
        assert_eq!(short_machine("box'1.lan"), "box-1");
        assert_eq!(short_machine(""), "mac");
    }

    #[test]
    fn colours_are_used_only_when_they_read_as_colours() {
        assert_eq!(colour("colour203").as_deref(), Some("colour203"));
        assert_eq!(colour("color7").as_deref(), Some("color7"));
        assert_eq!(colour("#ff00aa").as_deref(), Some("#ff00aa"));
        assert_eq!(colour("brightred").as_deref(), Some("brightred"));
        for bad in [
            "colour256",
            "red,bg=blue",
            "#(touch pwned)",
            "colour1]#(id)",
            "#ff00a",
            "",
        ] {
            assert_eq!(colour(bad), None, "{bad}");
        }
    }

    #[test]
    fn nothing_a_host_sends_reaches_a_command() {
        let evil = "#(touch /tmp/pwned)";
        let segs = vec![
            seg("git", evil, evil, evil, evil),
            seg("agents", evil, "colour1", "colour2", "AGENT"),
            Segment {
                runs: vec![run(evil, evil, evil), run(evil, "colour1]#(id)", "red")],
                ..seg("chunk", evil, "", "", "")
            },
            seg("health", evil, "", "", ""),
        ];
        // Read the way tmux reads a format: `##` is a `#`, and any other `#`
        // starts something. The only things started are styles this side
        // wrote.
        let starts = |out: &str| -> Vec<char> {
            let mut seen = Vec::new();
            let mut chars = out.chars();
            while let Some(c) = chars.next() {
                if c == '#' {
                    match chars.next() {
                        Some('#') => {}
                        Some(n) => seen.push(n),
                        None => seen.push(' '),
                    }
                }
            }
            seen
        };
        let out = render(evil, Some(&segs), true, &[evil.to_string()]);
        assert_eq!(out.matches("##(touch").count(), 7, "{out}");
        assert!(starts(&out).iter().all(|c| *c == '['), "{out}");
        let down = render(evil, Some(&segs), false, &[evil.to_string()]);
        assert!(starts(&down).iter().all(|c| *c == '['), "{down}");
    }

    fn win(index: u32, name: &str, active: bool, bell: bool) -> crate::link::Window {
        crate::link::Window {
            index,
            name: name.into(),
            active,
            bell,
        }
    }

    #[test]
    fn the_hosts_windows_are_one_entry_in_the_lists_own_styles() {
        let wins = [
            win(1, "ai", false, false),
            win(2, "dynamic", true, false),
            win(3, "run", false, true),
        ];
        assert_eq!(
            render_windows(&wins, "default", "fg=red,bold", " "),
            "#[default]1:ai#[default] #[default]#[fg=red,bold]2:dynamic*#[default] #[default]3:run!#[default]"
        );
        // A name is text: the `#` doubled, so tmux draws it and runs nothing.
        let evil = render_windows(
            &[win(1, "#(touch pwned)#[fg=red]", true, false)],
            "",
            "",
            "",
        );
        assert_eq!(evil, "#[default]1:##(touch pwned)##[fg=red]*#[default]");
    }

    #[test]
    fn a_saved_style_wins_over_the_blanked_one() {
        // Nothing saved: the empty option, then the window's own three.
        let (st, taken) = ListStyle::read("\tfg=grey\tfg=red,bold\t ");
        assert!(!taken);
        assert_eq!(
            st,
            ListStyle {
                normal: "fg=grey".into(),
                current: "fg=red,bold".into(),
                sep: " ".into()
            }
        );
        // Blanked by an earlier daemon: the saved copy is the window's own.
        let (st, taken) = ListStyle::read("fg=grey\tfg=red,bold\t \tdefault\tdefault\t ");
        assert!(taken);
        assert_eq!(st.current, "fg=red,bold");
        let cmds = release_commands("api@vm", &st);
        assert!(cmds.iter().any(|c| c.ends_with(&[
            "window-status-current-style".to_string(),
            "fg=red,bold".to_string()
        ])));
    }

    #[test]
    fn the_bar_says_each_part_in_words() {
        let segs = vec![
            seg("git", "main, 2 ahead", "", "", "BRANCH"),
            seg("agents", "1 agent, 1 waiting", "", "", "AGENT"),
            seg("health", "config changed", "", "", ""),
        ];
        let out = render("vm", Some(&segs), true, &["offline".into()]);
        let words: String = out
            .split("#[")
            .map(|p| p.split_once(']').map_or(p, |(_, t)| t))
            .collect();
        assert!(words.contains("main, 2 ahead"), "{words}");
        assert!(words.contains("vm: config changed"), "{words}");
        assert!(words.contains("here: offline"), "{words}");
        assert!(out.ends_with("#[default]"));
        let down = render("vm", Some(&segs), false, &[]);
        assert!(down.contains("vm: no link"), "{down}");
        assert!(!down.contains("main"), "nothing old shown as now");
        assert_eq!(render("vm", None, true, &[]), "", "up, nothing heard yet");
    }
}
