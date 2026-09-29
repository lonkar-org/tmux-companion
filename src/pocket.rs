//! A pocket pane: a shell you pull out beside the editor, look at, and put
//! away without losing it.
//!
//! `run` slides a pane out for one command and takes it down when the command
//! exits; `zen` hides everything but the pane you are in. Neither keeps a pane
//! around between two presses of a key. This does: the first press splits a
//! shell off the side of the window, the second parks it in a window called
//! `_pocket`, and the third brings the same pane back, process, scrollback and
//! all, beside wherever you are now.
//!
//! The pane is told apart by a pane option, `@tmux-companion-pocket`, holding
//! its name, and not by its title: a shell that sets its own title would
//! rename the pocket out from under the key. The title is set as well,
//! because that is what the border and the `panes` picker show.
//!
//! `_pocket` is an ordinary window, since tmux has no hidden one. The places
//! that would be surprised by it know its name: `toggle` cycles past it, and
//! `project save` leaves it and every pocket pane out of the layout it
//! writes, because a scratch shell is not part of what a project opens with.

/// The window parked pockets live in, one per session.
pub const WINDOW: &str = "_pocket";

/// The pane option that marks a pocket and holds its name.
pub const MARK: &str = "@tmux-companion-pocket";

/// The pocket's name when none is given.
pub const DEFAULT_NAME: &str = "shell";

/// One pane of the session, as far as the pocket cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// `#{pane_id}`.
    pub id: String,
    /// `#{window_id}`.
    pub window: String,
    /// `#{window_name}`.
    pub window_name: String,
    /// `#{pane_width}`.
    pub width: u16,
    /// The pocket name it carries, empty for an ordinary pane.
    pub mark: String,
}

/// The `-F` string [`parse`] reads.
pub fn format() -> String {
    format!("#{{pane_id}}\t#{{window_id}}\t#{{window_name}}\t#{{pane_width}}\t#{{{MARK}}}")
}

/// Parse a `list-panes -s` in [`format()`]. A line that does not fit is
/// skipped.
pub fn parse(text: &str) -> Vec<Seen> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() != 5 || !f[0].starts_with('%') {
                return None;
            }
            Some(Seen {
                id: f[0].trim().to_string(),
                window: f[1].trim().to_string(),
                window_name: f[2].to_string(),
                width: f[3].trim().parse().unwrap_or(0),
                mark: f[4].trim().to_string(),
            })
        })
        .collect()
}

/// Where the key was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Here {
    /// The pane the key was pressed in.
    pub pane: String,
    /// Its window.
    pub window: String,
    /// That window's width, for the pocket's share of it.
    pub window_width: u16,
    /// The pane's directory, which is where a new pocket starts.
    pub cwd: String,
}

/// What one press does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Move {
    /// No pocket by this name yet: split one off.
    Create,
    /// It is parked, or out in another window: bring it here.
    Bring {
        /// The pocket pane.
        id: String,
    },
    /// It is out in this window: put it away.
    Park {
        /// The pocket pane.
        id: String,
        /// How wide it is now, for the slide shut.
        width: u16,
        /// The `_pocket` window to join, when the session has one.
        into: Option<String>,
    },
    /// The key was pressed inside the `_pocket` window itself, where there
    /// is nothing to bring a pane beside.
    Nothing,
}

/// Decide what a press means, from where it happened and what the session
/// holds.
pub fn decide(here: &Here, panes: &[Seen], name: &str) -> Move {
    let mine = panes.iter().find(|p| p.mark == name);
    let in_pocket_window = panes
        .iter()
        .any(|p| p.window == here.window && p.window_name == WINDOW);
    match mine {
        Some(p) if p.window == here.window && !in_pocket_window => Move::Park {
            id: p.id.clone(),
            width: p.width,
            // Another pane has to hold the window open, or there is no
            // window to join; the pocket itself being its only pane cannot
            // happen while the pocket is out here.
            into: panes
                .iter()
                .find(|o| o.window_name == WINDOW && o.id != p.id)
                .map(|o| o.window.clone()),
        },
        _ if in_pocket_window => Move::Nothing,
        Some(p) => Move::Bring { id: p.id.clone() },
        None => Move::Create,
    }
}

/// The `split-window` that makes a new pocket, printing its pane id.
pub fn create_args(here: &Here, opening: u16) -> Vec<String> {
    [
        "split-window",
        "-fh",
        "-t",
        &here.pane,
        "-l",
        &opening.to_string(),
        "-c",
        &here.cwd,
        "-P",
        "-F",
        "#{pane_id}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The commands that mark a new pane as the pocket called `name`.
pub fn mark_args(id: &str, name: &str) -> Vec<Vec<String>> {
    let s = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        s(&["set-option", "-p", "-t", id, MARK, name]),
        s(&["select-pane", "-t", id, "-T", name]),
    ]
}

/// The `join-pane` that brings a pocket beside the pane the key was pressed
/// in, full height at the window's edge, the way `run` opens its pane.
pub fn bring_args(here: &Here, id: &str, opening: u16) -> Vec<String> {
    [
        "join-pane",
        "-fh",
        "-l",
        &opening.to_string(),
        "-s",
        id,
        "-t",
        &here.pane,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The command that puts a pocket away: into the session's `_pocket` window
/// when there is one, into a new window by that name when there is not.
/// `-d` both ways, so the view stays where the key was pressed.
pub fn park_args(id: &str, into: Option<&str>) -> Vec<String> {
    let s = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match into {
        Some(window) => s(&["join-pane", "-d", "-s", id, "-t", window]),
        None => s(&["break-pane", "-d", "-s", id, "-n", WINDOW]),
    }
}

/// A project capture's listings with the pocket taken out: the `_pocket`
/// window's line, its panes, and any pocket pane that is out in another
/// window.
///
/// `windows` is one `#{window_index}\t#{window_name}\t...` line per window,
/// `panes` one `#{window_index}\t#{pane_index}\t...` line per pane, and
/// `marks` one `#{window_index}\t#{pane_index}\t#{window_name}\t<mark>` line
/// per pane. A window left with one pane by this still carries the layout
/// string of two, which the capture drops for a single pane anyway; one left
/// with two or more keeps a layout string that counts the pocket, and tmux
/// refuses that on restore and tiles the window instead. Parking the pocket
/// before saving is the way round it.
pub fn without_pocket(windows: &str, panes: &str, marks: &str) -> (String, String) {
    let mut gone_windows = std::collections::HashSet::new();
    let mut gone_panes = std::collections::HashSet::new();
    for line in windows.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 2 && f[1] == WINDOW {
            gone_windows.insert(f[0].trim().to_string());
        }
    }
    for line in marks.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 4 {
            continue;
        }
        if f[2] == WINDOW {
            gone_windows.insert(f[0].trim().to_string());
        }
        if !f[3].trim().is_empty() {
            gone_panes.insert((f[0].trim().to_string(), f[1].trim().to_string()));
        }
    }
    let keep = |text: &str, gone: &dyn Fn(&[&str]) -> bool| {
        let mut out = String::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if !gone(&f) {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    };
    let windows = keep(windows, &|f| {
        f.first().is_some_and(|w| gone_windows.contains(w.trim()))
    });
    let panes = keep(panes, &|f| {
        f.first().is_some_and(|w| gone_windows.contains(w.trim()))
            || (f.len() >= 2
                && gone_panes.contains(&(f[0].trim().to_string(), f[1].trim().to_string())))
    });
    (windows, panes)
}

/// `pocket`: one press of the key.
///
/// `pane` is the pane the key was pressed in, from the binding; without it
/// `$TMUX_PANE` stands in, which is right at a shell and wrong under
/// `run-shell`, the same as for `zen`.
pub async fn run(name: Option<String>, pane: Option<String>) -> anyhow::Result<()> {
    use crate::cli::{tmux, tmux_capture, tmux_display_at};

    let config = crate::cli::config_or_default();
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| DEFAULT_NAME.to_string());
    let pane = pane
        .filter(|p| !p.trim().is_empty())
        .or_else(crate::cli::pane_target);
    let answered = tmux_display_at(
        pane.as_deref(),
        "#{pane_id}\t#{window_id}\t#{window_width}\t#{session_name}\t#{pane_current_path}",
    )
    .await;
    let f: Vec<&str> = answered.split('\t').collect();
    if f.len() != 5 || !f[0].starts_with('%') {
        anyhow::bail!("not inside tmux, so there is no window to open a pocket in");
    }
    let here = Here {
        pane: f[0].to_string(),
        window: f[1].to_string(),
        window_width: f[2].trim().parse().unwrap_or(180),
        cwd: f[4].to_string(),
    };
    let session = format!("={}", f[3]);
    let listing = tmux_capture(&["list-panes", "-s", "-t", &session, "-F", &format()]).await;

    // No slide for a pocket, unlike `run`'s pane. A pane opened one column
    // wide gets its shell's first prompt drawn one character to a row, and
    // zsh's redraws go on believing it; the next resize sends the cursor up
    // that many rows and clears everything below, which wiped what the pocket
    // had printed. Parking and bringing back had the same problem from the
    // other side. A pocket is an interactive shell, so it opens, parks and
    // comes back at its own width.
    let width = crate::run::pane_width(here.window_width, config.run.width_percent);
    let run = |args: Vec<String>| async move {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        tmux(&borrowed).await;
    };

    match decide(&here, &parse(&listing), &name) {
        Move::Create => {
            let args = create_args(&here, width);
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            let id = tmux_capture(&borrowed).await.trim().to_string();
            if !id.starts_with('%') {
                anyhow::bail!("tmux would not split the window for a pocket");
            }
            for args in mark_args(&id, &name) {
                run(args).await;
            }
        }
        Move::Bring { id } => {
            run(bring_args(&here, &id, width)).await;
            tmux(&["select-pane", "-t", &id]).await;
        }
        Move::Park { id, into, .. } => {
            match into {
                Some(window) => run(park_args(&id, Some(&window))).await,
                None => {
                    // The window's id comes back so its name can be held:
                    // left to automatic-rename it would be called `zsh` by
                    // the next prompt and nothing would find it again.
                    let mut args = park_args(&id, None);
                    args.extend(["-P", "-F", "#{window_id}"].map(String::from));
                    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
                    let window = tmux_capture(&borrowed).await.trim().to_string();
                    if window.starts_with('@') {
                        tmux(&["set-option", "-w", "-t", &window, "automatic-rename", "off"]).await;
                        tmux(&["set-option", "-w", "-t", &window, "allow-rename", "off"]).await;
                    }
                }
            }
        }
        Move::Nothing => {
            tmux(&[
                "display-message",
                "tmux-companion: this window is where pockets are parked; press the key in another",
            ])
            .await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here() -> Here {
        Here {
            pane: "%1".into(),
            window: "@1".into(),
            window_width: 200,
            cwd: "/w/api".into(),
        }
    }

    fn seen(id: &str, window: &str, name: &str, mark: &str) -> Seen {
        Seen {
            id: id.into(),
            window: window.into(),
            window_name: name.into(),
            width: 66,
            mark: mark.into(),
        }
    }

    #[test]
    fn the_listing_parses_and_an_unset_mark_is_empty() {
        assert_eq!(format().split('\t').count(), 5);
        let panes = parse("%1\t@1\tedit\t133\t\n%7\t@4\t_pocket\t66\tshell\nnonsense\n");
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].mark, "");
        assert_eq!(panes[1].mark, "shell");
        assert_eq!(panes[1].width, 66);
        assert_eq!(panes[1].window_name, WINDOW);
    }

    #[test]
    fn the_first_press_creates_the_second_parks_and_the_third_brings_it_back() {
        let h = here();
        // Nothing marked: make one.
        let fresh = vec![seen("%1", "@1", "edit", "")];
        assert_eq!(decide(&h, &fresh, "shell"), Move::Create);
        // Out beside the editor: put it away, in a new window the first time.
        let out = vec![
            seen("%1", "@1", "edit", ""),
            seen("%7", "@1", "edit", "shell"),
        ];
        assert_eq!(
            decide(&h, &out, "shell"),
            Move::Park {
                id: "%7".into(),
                width: 66,
                into: None
            }
        );
        // Parked: bring it here.
        let parked = vec![
            seen("%1", "@1", "edit", ""),
            seen("%7", "@4", WINDOW, "shell"),
        ];
        assert_eq!(
            decide(&h, &parked, "shell"),
            Move::Bring { id: "%7".into() }
        );
        // Out in another window of the session: it comes to this one.
        let elsewhere = vec![
            seen("%1", "@1", "edit", ""),
            seen("%2", "@2", "ai", ""),
            seen("%7", "@2", "ai", "shell"),
        ];
        assert_eq!(
            decide(&h, &elsewhere, "shell"),
            Move::Bring { id: "%7".into() }
        );
    }

    #[test]
    fn a_second_pocket_joins_the_window_the_first_is_parked_in() {
        let h = here();
        let panes = vec![
            seen("%1", "@1", "edit", ""),
            seen("%7", "@4", WINDOW, "shell"),
            seen("%8", "@1", "edit", "logs"),
        ];
        assert_eq!(
            decide(&h, &panes, "logs"),
            Move::Park {
                id: "%8".into(),
                width: 66,
                into: Some("@4".into())
            }
        );
        // Names are told apart: `logs` out here does not make `shell` a park.
        assert_eq!(decide(&h, &panes, "shell"), Move::Bring { id: "%7".into() });
    }

    #[test]
    fn a_press_inside_the_pocket_window_does_nothing() {
        let h = Here {
            pane: "%7".into(),
            window: "@4".into(),
            ..here()
        };
        let panes = vec![
            seen("%1", "@1", "edit", ""),
            seen("%7", "@4", WINDOW, "shell"),
        ];
        assert_eq!(decide(&h, &panes, "shell"), Move::Nothing);
        assert_eq!(decide(&h, &panes, "other"), Move::Nothing);
    }

    #[test]
    fn the_commands_target_the_pane_the_key_was_pressed_in() {
        let h = here();
        let create = create_args(&h, 1);
        assert_eq!(create[..6], ["split-window", "-fh", "-t", "%1", "-l", "1"]);
        assert!(create.windows(2).any(|w| w == ["-c", "/w/api"]));
        assert_eq!(
            bring_args(&h, "%7", 1),
            ["join-pane", "-fh", "-l", "1", "-s", "%7", "-t", "%1"]
        );
        assert_eq!(
            park_args("%7", None),
            ["break-pane", "-d", "-s", "%7", "-n", WINDOW]
        );
        assert_eq!(
            park_args("%7", Some("@4")),
            ["join-pane", "-d", "-s", "%7", "-t", "@4"]
        );
        let marks = mark_args("%7", "shell");
        assert_eq!(marks[0], ["set-option", "-p", "-t", "%7", MARK, "shell"]);
        assert_eq!(marks[1], ["select-pane", "-t", "%7", "-T", "shell"]);
    }

    #[test]
    fn a_capture_loses_the_pocket_window_and_a_pocket_that_is_out() {
        let windows =
            "1\tedit\t200\t50\tlayout-a\n2\tai\t200\t50\tlayout-b\n3\t_pocket\t200\t50\tlayout-c\n";
        let panes = "1\t0\t/w\tnvim\t\n1\t1\t/w\tzsh\t\n2\t0\t/w\tclaude\t\n3\t0\t/w\tzsh\t\n";
        let marks = "1\t0\tedit\t\n1\t1\tedit\tshell\n2\t0\tai\t\n3\t0\t_pocket\tlogs\n";
        let (w, p) = without_pocket(windows, panes, marks);
        assert_eq!(w, "1\tedit\t200\t50\tlayout-a\n2\tai\t200\t50\tlayout-b\n");
        assert_eq!(p, "1\t0\t/w\tnvim\t\n2\t0\t/w\tclaude\t\n");
        // The window goes by its name even when the marks could not be read.
        let (w, p) = without_pocket(windows, panes, "");
        assert!(!w.contains(WINDOW), "{w}");
        assert_eq!(p.lines().count(), 3, "{p}");
        // No pocket anywhere: nothing changes.
        let two = "1\tedit\t200\t50\tlayout-a\n2\tai\t200\t50\tlayout-b\n";
        let plain = "1\t0\tedit\t\n2\t0\tai\t\n";
        let (w, p) = without_pocket(two, "1\t0\t/w\tnvim\t\n", plain);
        assert_eq!(w, two);
        assert_eq!(p, "1\t0\t/w\tnvim\t\n");
    }
}
