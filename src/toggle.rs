//! which window `toggle` moves to: the one you were on before, then on by index the way that jump went
//!
//! The default order is the Alt-Tab shape. The first press goes to tmux's last
//! window. A press inside `[toggle] chain_ms` of the one before keeps going by
//! index in the direction the first jump went, wrapping at the ends, so a last
//! window to the left sweeps left and one to the right sweeps right. When the
//! sweep stops, the window it started from is left as tmux's last window, so
//! the next first press goes back there, the way Alt-Tab goes back to where you
//! were before you started tabbing.
//!
//! A terminal never sees a key released, so the time between presses stands in
//! for letting go of Alt. That makes the sweep timed; `order = "right"` and
//! `"left"` are the same key with no clock in it, and `toggle --last` is the
//! flip on its own.
//!
//! The sweep is kept in the session option `@tc_toggle`, because every press
//! is a new process and the session is what the key was pressed in.
//!
//! The announcement waits for the presses to stop. tmux draws a message over
//! the whole status line, window list and all, so a message on every press hid
//! the very list a sweep is watched on until a key cleared it. Each press now
//! writes its stamp to `@tc_toggle_press` and asks tmux to look again
//! `chain_ms` later; only the press nothing has followed says where it landed.

use crate::cli::tmux;
use crate::config::{Toggle as Settings, ToggleOrder};

/// What `list-windows -F` is asked for. The name is last because it may hold
/// spaces.
pub const LIST_FORMAT: &str =
    "#{window_index} #{window_active} #{window_last_flag} #{window_id} #{window_name}";

/// The session option a sweep in progress is kept in.
pub const OPTION: &str = "@tc_toggle";

/// The session option holding the stamp of the latest press, which the
/// delayed announcement checks is still its own.
pub const PRESS: &str = "@tc_toggle_press";

/// The session option holding what the latest press would say, so the
/// delayed command names no window and needs no quoting for one.
pub const SAID: &str = "@tc_toggle_said";

/// One window of the session, as [`LIST_FORMAT`] lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub index: u32,
    pub id: String,
    pub active: bool,
    pub last: bool,
    pub name: String,
}

/// Which way a sweep goes by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    Right,
    Left,
}

/// A sweep in progress: where it started, which way it goes, which window it
/// is on, and when the last press was, in milliseconds since the epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub origin: String,
    pub way: Way,
    pub at: String,
    pub ms: u64,
}

impl Chain {
    /// `@3 right @5 1790067600000`, which is what [`OPTION`] holds.
    pub fn encode(&self) -> String {
        let way = match self.way {
            Way::Right => "right",
            Way::Left => "left",
        };
        format!("{} {way} {} {}", self.origin, self.at, self.ms)
    }

    /// The other half of [`Chain::encode`]. Anything else, an unset option
    /// included, is no sweep.
    pub fn decode(s: &str) -> Option<Self> {
        let mut f = s.split_whitespace();
        let origin = f.next()?.to_string();
        let way = match f.next()? {
            "right" => Way::Right,
            "left" => Way::Left,
            _ => return None,
        };
        let at = f.next()?.to_string();
        let ms = f.next()?.parse().ok()?;
        Some(Self {
            origin,
            way,
            at,
            ms,
        })
    }
}

/// What one press does.
#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    /// The window to go to.
    pub target: Window,
    /// A window to select on the way, so tmux remembers it as the last one:
    /// the start of the sweep, when the window being left is not it.
    pub via: Option<String>,
    /// The sweep to keep for the next press, `None` outside `recent`.
    pub chain: Option<Chain>,
    /// Where the target sits among the windows the key moves through,
    /// counting from one, and how many there are.
    pub place: (usize, usize),
}

/// The session's windows in index order, read from [`LIST_FORMAT`] lines.
///
/// Windows are told apart by index and id, never by name: two windows called
/// `zsh` are two stops. The window pockets are parked in is not one to move
/// into, so it is left out unless it is where the key was pressed.
pub fn parse(list: &str) -> Vec<Window> {
    let mut windows: Vec<Window> = list
        .lines()
        .filter_map(|l| {
            let mut f = l.trim().splitn(5, ' ');
            let index = f.next()?.parse().ok()?;
            let active = f.next()? == "1";
            let last = f.next()? == "1";
            let id = f.next()?.to_string();
            let name = f.next().unwrap_or_default().to_string();
            if !active && name == crate::pocket::WINDOW {
                return None;
            }
            Some(Window {
                index,
                id,
                active,
                last,
                name,
            })
        })
        .collect();
    windows.sort_by_key(|w| w.index);
    windows
}

/// The window one step from the active one, wrapping at the ends.
fn step(windows: &[Window], way: Way) -> Option<usize> {
    let here = windows.iter().position(|w| w.active)?;
    let n = windows.len();
    Some(match way {
        Way::Right => (here + 1) % n,
        Way::Left => (here + n - 1) % n,
    })
}

/// What a press does, given the windows, the order, the sweep left by the
/// last press, and the time now. `None` means there is nowhere to go: one
/// window, or no active one.
pub fn plan(
    windows: &[Window],
    order: ToggleOrder,
    chain: Option<&Chain>,
    now: u64,
    chain_ms: u64,
) -> Option<Move> {
    if windows.len() < 2 {
        return None;
    }
    let active = windows.iter().find(|w| w.active)?;
    let done = |i: usize, via: Option<String>, chain: Option<Chain>| Move {
        target: windows[i].clone(),
        via,
        chain,
        place: (i + 1, windows.len()),
    };
    match order {
        ToggleOrder::Right => return step(windows, Way::Right).map(|i| done(i, None, None)),
        ToggleOrder::Left => return step(windows, Way::Left).map(|i| done(i, None, None)),
        ToggleOrder::Recent => (),
    }
    // On with the sweep: the last press was this one's, recently, and nothing
    // moved the session somewhere else since.
    if let Some(c) = chain.filter(|c| c.at == active.id && now >= c.ms && now - c.ms < chain_ms) {
        let i = step(windows, c.way)?;
        let target = &windows[i];
        // Through the start of the sweep on the way, so tmux's last window is
        // where the sweep began, unless that is the window being left or the
        // one being gone to, when the plain move already says the same.
        let via = (c.origin != active.id
            && c.origin != target.id
            && windows.iter().any(|w| w.id == c.origin))
        .then(|| c.origin.clone());
        let next = Chain {
            origin: c.origin.clone(),
            way: c.way,
            at: target.id.clone(),
            ms: now,
        };
        return Some(done(i, via, Some(next)));
    }
    // A new sweep: the last window, and the way it lies sets the way on. With
    // no last window, or only the parked pocket, the next one to the right.
    let (i, way) = match windows.iter().position(|w| w.last && !w.active) {
        Some(i) if windows[i].index > active.index => (i, Way::Right),
        Some(i) => (i, Way::Left),
        None => (step(windows, Way::Right)?, Way::Right),
    };
    let next = Chain {
        origin: active.id.clone(),
        way,
        at: windows[i].id.clone(),
        ms: now,
    };
    Some(done(i, None, Some(next)))
}

/// What the message line says on arrival: the window's name and its place.
pub fn announcement(m: &Move) -> String {
    let (at, of) = m.place;
    format!("{}, {at} of {of}", m.target.name)
}

/// The `run-shell` arguments that say [`SAID`] `delay_ms` from now, if the
/// press stamped `stamp` is still the latest one in the session and the
/// session is still on `target`.
///
/// `run-shell` expands formats once when it is run, so the ones meant for the
/// moment the delay is up are written `##{`. The session is named by its id,
/// `$3`, which needs no quoting, and the window name never appears in the
/// command at all.
pub fn announce_later(session_id: &str, stamp: u64, target: &str, delay_ms: u64) -> Vec<String> {
    let at = format!("{session_id}:");
    let check = format!(
        "if-shell -F -t '{at}' \
         '##{{&&:##{{==:##{{{PRESS}}},{stamp}}},##{{==:##{{window_id}},{target}}}}}' \
         {{ display-message -t '{at}' '##{{{SAID}}}' }}"
    );
    vec![
        "run-shell".to_string(),
        "-b".to_string(),
        "-C".to_string(),
        "-d".to_string(),
        format!("{}.{:03}", delay_ms / 1000, delay_ms % 1000),
        "-t".to_string(),
        at,
        check,
    ]
}

/// Milliseconds since the epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One press of the key in `session`.
pub async fn run(session: &str, settings: &Settings) -> anyhow::Result<()> {
    let out = crate::tmux::command()
        .args([
            "list-windows",
            "-t",
            &format!("={session}"),
            "-F",
            LIST_FORMAT,
        ])
        .output()
        .await?;
    let windows = parse(&String::from_utf8_lossy(&out.stdout));
    // The session's id for the delayed announcement, and the sweep, in one
    // call. With the colon: `-t` takes a pane, and a bare `=name` is no pane
    // on tmux 3.4.
    let out = crate::tmux::command()
        .args([
            "display-message",
            "-p",
            "-t",
            &format!("={session}:"),
            &format!("#{{session_id}} #{{{OPTION}}}"),
        ])
        .output()
        .await?;
    let here = String::from_utf8_lossy(&out.stdout);
    let (session_id, chain) = here.trim().split_once(' ').unwrap_or((here.trim(), ""));
    let chain = match settings.order {
        ToggleOrder::Recent => Chain::decode(chain),
        _ => None,
    };
    let now = now_ms();
    let Some(m) = plan(
        &windows,
        settings.order,
        chain.as_ref(),
        now,
        settings.chain_ms,
    ) else {
        return Ok(());
    };
    // One tmux call, so nothing is drawn between the stop on the way and the
    // window gone to.
    let mut args: Vec<String> = Vec::new();
    let mut then = |a: &[&str]| {
        if !args.is_empty() {
            args.push(";".to_string());
        }
        args.extend(a.iter().map(|s| s.to_string()));
    };
    if let Some(via) = &m.via {
        then(&["select-window", "-t", &format!("={session}:{via}")]);
    }
    then(&[
        "select-window",
        "-t",
        &format!("={session}:{}", m.target.id),
    ]);
    if let Some(c) = &m.chain {
        then(&[
            "set-option",
            "-t",
            // With the colon: `set-option -t` takes a pane, and a bare
            // `=name` is no pane on tmux 3.4.
            &format!("={session}:"),
            OPTION,
            &c.encode(),
        ]);
    }
    if settings.announce && session_id.starts_with('$') {
        let at = format!("{session_id}:");
        then(&["set-option", "-t", &at, PRESS, &now.to_string()]);
        then(&["set-option", "-t", &at, SAID, &announcement(&m)]);
        let later = announce_later(session_id, now, &m.target.id, settings.chain_ms);
        let later: Vec<&str> = later.iter().map(String::as_str).collect();
        then(&later);
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    tmux(&borrowed).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows from a picture like the one in the issue: `a* b c- d`, where
    /// `*` is active and `-` is last. Index 1 upwards, id `@` and the index.
    fn session(picture: &str) -> Vec<Window> {
        let list: String = picture
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| {
                let name = w.trim_end_matches(['*', '-']);
                format!(
                    "{} {} {} @{} {name}\n",
                    i + 1,
                    u8::from(w.ends_with('*')),
                    u8::from(w.ends_with('-')),
                    i + 1
                )
            })
            .collect();
        parse(&list)
    }

    /// Press the key in `recent` order and apply what it does the way tmux
    /// would: the stop on the way, then the target, each leaving the window
    /// before it as tmux's last. Returns the new windows and the sweep.
    fn press(windows: &[Window], chain: Option<&Chain>, now: u64) -> (Vec<Window>, Option<Chain>) {
        let m = plan(windows, ToggleOrder::Recent, chain, now, 800).expect("somewhere to go");
        let mut w = windows.to_vec();
        let mut go = |id: &str| {
            let from = w.iter().position(|x| x.active).unwrap();
            if w[from].id == id {
                return;
            }
            for x in w.iter_mut() {
                x.last = false;
            }
            w[from].active = false;
            w[from].last = true;
            let to = w.iter().position(|x| x.id == id).unwrap();
            w[to].active = true;
        };
        if let Some(via) = &m.via {
            go(via);
        }
        go(&m.target.id);
        (w, m.chain)
    }

    fn picture(windows: &[Window]) -> String {
        windows
            .iter()
            .map(|w| {
                let mark = if w.active {
                    "*"
                } else if w.last {
                    "-"
                } else {
                    ""
                };
                format!("{}{mark}", w.name)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_last_window_to_the_left_sweeps_left() {
        // c active, a last: a, then d, c and b inside the delay.
        let w = session("a- b c* d");
        let (w, c) = press(&w, None, 1_000);
        assert_eq!(picture(&w), "a* b c- d");
        let (w, c) = press(&w, c.as_ref(), 1_500);
        assert_eq!(picture(&w), "a b c- d*");
        let (w, c) = press(&w, c.as_ref(), 2_000);
        assert_eq!(w.iter().find(|x| x.active).unwrap().name, "c");
        let (w, _) = press(&w, c.as_ref(), 2_500);
        // b, with c, where the sweep began, as the last window.
        assert_eq!(picture(&w), "a b* c- d");
    }

    #[test]
    fn a_last_window_to_the_right_sweeps_right() {
        let w = session("a b* c- d");
        let (w, c) = press(&w, None, 10_000);
        assert_eq!(picture(&w), "a b- c* d");
        let (w, c) = press(&w, c.as_ref(), 10_400);
        assert_eq!(picture(&w), "a b- c d*");
        let (w, _) = press(&w, c.as_ref(), 10_800);
        assert_eq!(picture(&w), "a* b- c d");
    }

    #[test]
    fn a_press_after_the_delay_starts_again_from_the_last_window() {
        let w = session("a b* c- d");
        let (w, c) = press(&w, None, 0);
        let (w, c) = press(&w, c.as_ref(), 500);
        assert_eq!(picture(&w), "a b- c d*");
        // 800 ms after the last press is too late: back to b, the last window.
        let (w, _) = press(&w, c.as_ref(), 1_300);
        assert_eq!(picture(&w), "a b* c d-");
    }

    #[test]
    fn a_sweep_left_by_another_key_is_not_carried_on() {
        // The sweep says it is on d, but a was selected some other way.
        let w = session("a* b c- d");
        let stale = Chain {
            origin: "@2".into(),
            way: Way::Right,
            at: "@4".into(),
            ms: 0,
        };
        let m = plan(&w, ToggleOrder::Recent, Some(&stale), 100, 800).unwrap();
        assert_eq!(m.target.name, "c");
        assert_eq!(m.via, None);
    }

    #[test]
    fn two_windows_flip_whatever_the_timing() {
        let w = session("a* b-");
        let (w, c) = press(&w, None, 0);
        assert_eq!(picture(&w), "a- b*");
        let (w, c) = press(&w, c.as_ref(), 100);
        assert_eq!(picture(&w), "a* b-");
        let (w, _) = press(&w, c.as_ref(), 5_000);
        assert_eq!(picture(&w), "a- b*");
    }

    #[test]
    fn no_last_window_starts_to_the_right() {
        let w = session("a b* c");
        let m = plan(&w, ToggleOrder::Recent, None, 0, 800).unwrap();
        assert_eq!(m.target.name, "c");
        assert_eq!(m.chain.unwrap().way, Way::Right);
    }

    #[test]
    fn a_clock_that_went_backwards_starts_a_new_sweep() {
        let w = session("a b- c* d");
        let c = Chain {
            origin: "@2".into(),
            way: Way::Right,
            at: "@3".into(),
            ms: 5_000,
        };
        let m = plan(&w, ToggleOrder::Recent, Some(&c), 4_000, 800).unwrap();
        assert_eq!(m.target.name, "b");
    }

    #[test]
    fn right_and_left_cycle_by_index_with_no_clock() {
        let w = session("a b* c");
        let go = |order| plan(&w, order, None, 0, 800).unwrap();
        assert_eq!(go(ToggleOrder::Right).target.name, "c");
        assert_eq!(go(ToggleOrder::Left).target.name, "a");
        assert_eq!(go(ToggleOrder::Right).chain, None);
        let end = session("a b c*");
        assert_eq!(
            plan(&end, ToggleOrder::Right, None, 0, 800)
                .unwrap()
                .target
                .name,
            "a"
        );
        let start = session("a* b c");
        assert_eq!(
            plan(&start, ToggleOrder::Left, None, 0, 800)
                .unwrap()
                .target
                .name,
            "c"
        );
    }

    #[test]
    fn the_parked_pocket_is_passed_unless_it_is_where_you_are() {
        let list = "1 0 0 @1 edit\n2 1 0 @2 ai\n3 0 1 @3 _pocket\n4 0 0 @4 my logs\n";
        let w = parse(list);
        assert_eq!(w.len(), 3);
        assert_eq!(w[2].name, "my logs");
        // The pocket was the last window, so there is no last to go to.
        let m = plan(&w, ToggleOrder::Recent, None, 0, 800).unwrap();
        assert_eq!(m.target.name, "my logs");
        let inside = parse("1 0 1 @1 edit\n3 1 0 @3 _pocket\n");
        let m = plan(&inside, ToggleOrder::Recent, None, 0, 800).unwrap();
        assert_eq!(m.target.name, "edit");
    }

    #[test]
    fn gaps_in_the_numbering_are_followed_in_order() {
        // A closed window leaves a hole, and tmux lists in its own order.
        let w = parse("7 0 0 @7 x\n0 1 0 @0 y\n3 0 0 @3 z\n");
        assert_eq!(
            plan(&w, ToggleOrder::Right, None, 0, 0)
                .unwrap()
                .target
                .index,
            3
        );
        assert_eq!(
            plan(&w, ToggleOrder::Left, None, 0, 0)
                .unwrap()
                .target
                .index,
            7
        );
    }

    #[test]
    fn one_window_or_none_goes_nowhere() {
        assert_eq!(
            plan(&session("a*"), ToggleOrder::Recent, None, 0, 800),
            None
        );
        assert_eq!(plan(&[], ToggleOrder::Right, None, 0, 800), None);
    }

    #[test]
    fn the_sweep_reads_back_what_it_wrote() {
        let c = Chain {
            origin: "@3".into(),
            way: Way::Left,
            at: "@12".into(),
            ms: 1_790_067_600_000,
        };
        assert_eq!(Chain::decode(&c.encode()), Some(c));
        assert_eq!(Chain::decode(""), None);
        assert_eq!(Chain::decode("@3 up @4 1"), None);
    }

    #[test]
    fn the_announcement_waits_and_checks_it_is_still_the_latest_press() {
        let a = announce_later("$3", 1_790_067_600_123, "@7", 800);
        assert_eq!(
            a[..7],
            ["run-shell", "-b", "-C", "-d", "0.800", "-t", "$3:"]
        );
        assert_eq!(
            a[7],
            "if-shell -F -t '$3:' \
             '##{&&:##{==:##{@tc_toggle_press},1790067600123},##{==:##{window_id},@7}}' \
             { display-message -t '$3:' '##{@tc_toggle_said}' }"
        );
        assert_eq!(announce_later("$0", 0, "@1", 1_250)[4], "1.250");
        assert_eq!(announce_later("$0", 0, "@1", 0)[4], "0.000");
    }

    #[test]
    fn the_announcement_is_the_name_and_the_place() {
        let w = session("edit ai* logs");
        let m = plan(&w, ToggleOrder::Right, None, 0, 800).unwrap();
        assert_eq!(announcement(&m), "logs, 3 of 3");
    }
}
