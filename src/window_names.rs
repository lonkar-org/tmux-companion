//! Naming windows after what is running in them, from the job table.
//!
//! `[[sh_jobs.job]]` already maps a process-name pattern to an icon for the
//! status bar. Giving a row a `window_name` makes the same row answer a second
//! question, which is what a window holding that process should be called, and
//! the scan is one tmux call rather than the process-table walk the bar pays
//! for.
//!
//! Credit: `ofirgall/tmux-window-name` is this idea and it is a Python daemon,
//! so this removes a language runtime as well as a process, and
//! `joshmedeski/tmux-nerd-font-window-name` is the icon half. Both are alive
//! and worth installing if you are not running this.
//!
//! What it will not do is take a name away from somebody who set one. A window
//! with `automatic-rename` off was pinned deliberately, by `hold_name` in a
//! layout or by hand, and this leaves it alone unless it was the one that
//! pinned it.

use crate::config::ShJobs;

/// The window option this sets on a window it named, so a later pass can tell
/// its own work from somebody else's.
pub const MARKER: &str = "@tmux-companion-named";

/// One window as tmux reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRow {
    /// `#{window_id}`, which survives renumbering.
    pub id: String,
    /// `#{window_name}`.
    pub name: String,
    /// `#{pane_current_command}` of the active pane.
    pub command: String,
    /// Whether `automatic-rename` is on.
    pub automatic: bool,
    /// Whether this window carries [`MARKER`].
    pub ours: bool,
}

/// What a pass decides to do about one window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Give it this name and stop tmux renaming it.
    Rename(String, String),
    /// Hand it back: tmux renames it again and the marker goes.
    Release(String),
}

/// Parse `list-windows -a -F` in the format [`window_format`] asks for.
pub fn parse_windows(text: &str) -> Vec<WindowRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(5, '\t').collect();
            (f.len() == 5).then(|| WindowRow {
                id: f[0].to_string(),
                name: f[1].to_string(),
                command: f[2].to_string(),
                automatic: f[3].trim() == "on" || f[3].trim() == "1",
                ours: !f[4].trim().is_empty(),
            })
        })
        .collect()
}

/// The `-F` string that produces what [`parse_windows`] reads.
pub fn window_format() -> String {
    format!(
        "#{{window_id}}\t#{{window_name}}\t#{{pane_current_command}}\t#{{automatic-rename}}\t#{{{MARKER}}}"
    )
}

/// The name the job table gives a window running `command`, if any.
///
/// First matching row wins, the same rule the icons use, so one table is read
/// one way.
pub fn name_for(command: &str, config: &ShJobs) -> Option<String> {
    config
        .job
        .iter()
        .find(|j| j.window_name.is_some() && j.matches(command))
        .and_then(|j| j.window_name.clone())
}

/// What to do about every window in one pass.
pub fn plan(rows: &[WindowRow], config: &ShJobs) -> Vec<Action> {
    let mut out = Vec::new();
    for w in rows {
        match name_for(&w.command, config) {
            Some(name) => {
                // Somebody else pinned this name. `hold_name` in a layout does
                // exactly this, and an editor window called `edit` should not
                // start being called whatever the table says.
                if !w.automatic && !w.ours {
                    continue;
                }
                if w.name != name {
                    out.push(Action::Rename(w.id.clone(), name));
                }
            }
            // It matched last pass and does not now, so the name this put
            // there is stale. Handing it back is better than leaving a window
            // called `edit` with nothing running in it.
            None if w.ours => out.push(Action::Release(w.id.clone())),
            None => {}
        }
    }
    out
}

/// The tmux commands one action needs.
pub fn commands(action: &Action) -> Vec<Vec<String>> {
    let arg = |parts: &[&str]| parts.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    match action {
        Action::Rename(id, name) => vec![
            arg(&["rename-window", "-t", id, name]),
            arg(&["set-window-option", "-t", id, "automatic-rename", "off"]),
            arg(&["set-option", "-w", "-t", id, MARKER, "1"]),
        ],
        Action::Release(id) => vec![
            arg(&["set-window-option", "-t", id, "automatic-rename", "on"]),
            arg(&["set-option", "-w", "-t", id, "-u", MARKER]),
        ],
    }
}

/// The task: look at every window, rename what the table claims.
pub async fn window_names_loop(settings: crate::config::WindowNames, jobs: ShJobs) {
    let interval = std::time::Duration::from_secs(settings.interval_secs.max(1));
    loop {
        tokio::time::sleep(interval).await;
        let listing = tokio::process::Command::new("tmux")
            .args(["list-windows", "-a", "-F", &window_format()])
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
        for action in plan(&parse_windows(&listing), &jobs) {
            for cmd in commands(&action) {
                let _ = tokio::process::Command::new("tmux")
                    .args(&cmd)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::JobEntry;

    fn entry(pattern: &str, name: Option<&str>) -> JobEntry {
        JobEntry {
            match_: pattern.to_string(),
            icon: "x".to_string(),
            color: String::new(),
            window_name: name.map(str::to_string),
        }
    }

    fn table(entries: Vec<JobEntry>) -> ShJobs {
        ShJobs {
            job: entries,
            ..Default::default()
        }
    }

    fn row(name: &str, command: &str, automatic: bool, ours: bool) -> WindowRow {
        WindowRow {
            id: "@1".to_string(),
            name: name.to_string(),
            command: command.to_string(),
            automatic,
            ours,
        }
    }

    #[test]
    fn a_row_with_no_window_name_says_nothing_about_names() {
        // Every entry written before the field existed is one of these, and it
        // has to keep drawing its icon and nothing else.
        let t = table(vec![entry("nvim", None)]);
        assert_eq!(name_for("nvim", &t), None);
    }

    #[test]
    fn the_first_matching_row_wins_as_it_does_for_icons() {
        let t = table(vec![
            entry("^n", Some("first")),
            entry("nvim", Some("second")),
        ]);
        assert_eq!(name_for("nvim", &t).as_deref(), Some("first"));
    }

    #[test]
    fn a_row_without_a_name_does_not_shadow_a_later_one_that_has_it() {
        // The icon table and the name table are the same table, so an
        // icon-only row for nvim must not stop a naming row for nvim.
        let t = table(vec![entry("nvim", None), entry("nvim", Some("edit"))]);
        assert_eq!(name_for("nvim", &t).as_deref(), Some("edit"));
    }

    #[test]
    fn matching_is_the_same_regex_the_icons_use() {
        let t = table(vec![entry("^claude$", Some("ai"))]);
        assert_eq!(name_for("claude", &t).as_deref(), Some("ai"));
        assert_eq!(name_for("claudette", &t), None);
    }

    #[test]
    fn a_window_already_called_the_right_thing_is_left_alone() {
        // Renaming a window to what it is called already is a tmux call per
        // pass forever, and it fights nothing.
        let t = table(vec![entry("nvim", Some("edit"))]);
        assert!(plan(&[row("edit", "nvim", true, false)], &t).is_empty());
    }

    #[test]
    fn a_window_somebody_pinned_is_not_renamed() {
        // hold_name in a layout turns automatic-rename off, and a window
        // called `edit` on purpose should not start being called whatever the
        // table says.
        let t = table(vec![entry("nvim", Some("editor"))]);
        assert!(plan(&[row("mine", "nvim", false, false)], &t).is_empty());
    }

    #[test]
    fn a_window_this_named_before_can_be_renamed_again() {
        // It has automatic-rename off because this turned it off, so the
        // marker is what tells the two cases apart.
        let t = table(vec![entry("claude", Some("ai"))]);
        assert_eq!(
            plan(&[row("edit", "claude", false, true)], &t),
            vec![Action::Rename("@1".to_string(), "ai".to_string())]
        );
    }

    #[test]
    fn a_window_that_stopped_matching_is_handed_back_to_tmux() {
        let t = table(vec![entry("nvim", Some("edit"))]);
        assert_eq!(
            plan(&[row("edit", "zsh", false, true)], &t),
            vec![Action::Release("@1".to_string())]
        );
    }

    #[test]
    fn a_window_nobody_here_ever_touched_is_never_released() {
        let t = table(vec![entry("nvim", Some("edit"))]);
        assert!(plan(&[row("shell", "zsh", true, false)], &t).is_empty());
    }

    #[test]
    fn renaming_pins_the_name_and_leaves_the_marker() {
        let cmds = commands(&Action::Rename("@1".into(), "edit".into()));
        let joined: Vec<String> = cmds.iter().map(|c| c.join(" ")).collect();
        assert_eq!(joined[0], "rename-window -t @1 edit");
        assert!(joined[1].contains("automatic-rename off"), "{joined:?}");
        assert!(joined[2].contains(MARKER), "{joined:?}");
    }

    #[test]
    fn releasing_turns_renaming_back_on_and_unsets_the_marker() {
        let joined: Vec<String> = commands(&Action::Release("@1".into()))
            .iter()
            .map(|c| c.join(" "))
            .collect();
        assert!(joined[0].contains("automatic-rename on"), "{joined:?}");
        assert!(joined[1].contains("-u"), "{joined:?}");
    }

    #[test]
    fn the_format_and_the_parser_agree_on_their_field_count() {
        assert_eq!(window_format().matches('\t').count(), 4);
    }

    #[test]
    fn parsing_reads_both_spellings_of_an_option_being_on() {
        let rows = parse_windows("@1\tedit\tnvim\ton\t\n@2\tai\tclaude\t1\t1\n");
        assert!(rows[0].automatic && !rows[0].ours);
        assert!(rows[1].automatic && rows[1].ours);
    }

    #[test]
    fn a_window_name_containing_a_tab_cannot_break_the_parse() {
        // The command is the third field and the marker the fifth, so a name
        // with a tab in it would shift everything. splitn caps the damage at
        // the last field rather than dropping the row.
        let rows = parse_windows("@1\ta\tb\ton\t\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command, "b");
    }

    #[test]
    fn a_short_line_is_skipped_rather_than_panicking() {
        assert!(parse_windows("@1\tedit\n").is_empty());
    }
}
