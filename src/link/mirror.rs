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
            for s in segs {
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

/// How old a session listing the loop takes: mirror sessions open and close
/// far less often than a bar changes, so the listing can be the one another
/// loop (the inbox's every two seconds, the push loop's every one) already
/// read. A mirror session just opened gets its bar within this.
const LOOK: Duration = Duration::from_secs(5);

/// The daemon's mirror bar: once a tick, for each mirror session with a
/// client, the host's segments rendered and set as its `@tc-right` when they
/// changed. It reads the shared pane poll, which carries the session list,
/// up to [`LOOK`] old, so it starts no process of its own when another loop
/// has read one; with no host configured it reads nothing at all. With no
/// mirror open it looks once every [`LOOK`] rather than every tick.
pub async fn mirror_loop(state: Arc<Mutex<ServerState>>) {
    let machine = machine();
    let mut tick = crate::push::ticker();
    let mut last: HashMap<String, String> = HashMap::new();
    let mut looked: Option<Instant> = None;
    let mut any = false;
    loop {
        tick.tick().await;
        if state.lock().await.links.hosts.is_empty() {
            last.clear();
            continue;
        }
        if !any && looked.is_some_and(|t| t.elapsed() < LOOK) {
            continue;
        }
        looked = Some(Instant::now());
        let _ = crate::panes::shared(&state, LOOK).await;
        let mirrors = state.lock().await.mirrors.clone();
        any = !mirrors.is_empty();
        let open: Vec<String> = mirrors.iter().map(|m| m.host.clone()).collect();
        let shown: Vec<&super::Mirror> = mirrors.iter().filter(|m| m.attached > 0).collect();
        let wants: Vec<(String, String)> = shown
            .iter()
            .map(|m| (m.host.clone(), grouped_name(&m.remote, &machine)))
            .collect();
        state.lock().await.links.set_mirrors(&open, &wants);
        last.retain(|s, _| shown.iter().any(|m| &m.session == s));
        if shown.is_empty() {
            continue;
        }
        let here = crate::server::health_reasons(&state).await;
        let mut changed = Vec::new();
        for (m, (_, grouped)) in shown.iter().zip(&wants) {
            let (segs, up) = state.lock().await.links.segments_for(&m.host, grouped);
            let value = render(&m.host, segs.as_ref().map(|(s, _)| s.as_slice()), up, &here);
            if last.get(&m.session) != Some(&value) {
                last.insert(m.session.clone(), value.clone());
                changed.push((m.session.clone(), value));
            }
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

    fn seg(role: &str, text: &str, fg: &str, bg: &str, glyph: &str) -> Segment {
        Segment {
            role: role.into(),
            text: text.into(),
            fg: fg.into(),
            bg: bg.into(),
            glyph: glyph.into(),
        }
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
        assert_eq!(out.matches("##(touch").count(), 5, "{out}");
        assert!(starts(&out).iter().all(|c| *c == '['), "{out}");
        let down = render(evil, Some(&segs), false, &[evil.to_string()]);
        assert!(starts(&down).iter().all(|c| *c == '['), "{down}");
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
