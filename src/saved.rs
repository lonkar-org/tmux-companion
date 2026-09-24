//! Per-project layouts: the file a key writes and `project` reads back.
//!
//! A named `[[layout]]` in the config is a template somebody wrote. This is the
//! arrangement they ended up with, captured from a live session and written
//! next to the state directory rather than into their config, so the file the
//! tool rewrites is never the file they hand-maintain.
//!
//! Nothing here runs on a timer and nothing watches for a session closing.
//! `session-closed` fires after the windows are gone and there is nothing left
//! to read, so both triggers are a keypress: `project save`, and `project
//! close`, which captures before it exits.
//!
//! Capture is lossy and says so in the file it writes. A pane sitting at a
//! prompt reports the shell, which would restore as a shell inside a shell, and
//! a pane running `nvim src/config.rs` reports `nvim` with the arguments gone.
//! What it produces is a first draft of a layout, marked where it guessed.

use serde::{Deserialize, Serialize};

use crate::config::{Config, LayoutPane, LayoutWindow};

/// A layout captured from a live session.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct SavedLayout {
    /// The project directory this belongs to, which is the authority when the
    /// file name and the contents disagree.
    pub path: String,
    /// When it was captured, for the person reading the file a year later.
    #[serde(default)]
    pub captured_at: String,
    /// The width of the widest window at capture time.
    #[serde(default)]
    pub width: u32,
    /// The height of the tallest window at capture time.
    #[serde(default)]
    pub height: u32,
    /// The windows, in index order.
    #[serde(default)]
    pub window: Vec<LayoutWindow>,
}

/// Which file answered "what layout does this project get".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A file written by `project save` or `project close`.
    Saved(std::path::PathBuf),
    /// A `[[layout]]` in the config, by name.
    Named(String),
    /// Nothing matched, so the session is a plain shell.
    Nothing,
}

/// Where a project's saved layout lives under the state directory.
///
/// The directory separator is percent-encoded rather than flattened, so two
/// projects called `api` under different parents cannot land on one file, and
/// the name stays readable enough to delete by hand.
pub fn file_name(project_path: &str) -> String {
    let mut out = String::with_capacity(project_path.len() + 8);
    for ch in project_path.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            c => out.push(c),
        }
    }
    out.push_str(".toml");
    out
}

/// The saved-layout file for a project under a state directory.
pub fn path_in(state_dir: &std::path::Path, project_path: &str) -> std::path::PathBuf {
    state_dir.join("projects").join(file_name(project_path))
}

/// The saved-layout file for a project, or `None` with no state directory.
pub fn path_for(project_path: &str) -> Option<std::path::PathBuf> {
    crate::server::state_dir().map(|d| path_in(&d, project_path))
}

/// Read a project's saved layout, treating an unreadable, unparseable or empty
/// file as no layout at all.
///
/// A corrupt file costs the windows and not the session, the same bargain a
/// misspelled layout name already makes: somebody opening a project wants the
/// project, and a parse error in a cache is not their problem to solve at that
/// moment.
pub fn load(project_path: &str) -> Option<SavedLayout> {
    load_in(&crate::server::state_dir()?, project_path)
}

/// [`load`] from a named state directory.
///
/// The directory is a parameter so a test can hand over a temporary one
/// instead of setting `XDG_STATE_HOME`. Tests run in parallel threads of one
/// process, so an environment variable is shared mutable state between them,
/// which is a race waiting for a slow machine.
pub fn load_in(state_dir: &std::path::Path, project_path: &str) -> Option<SavedLayout> {
    let file = path_in(state_dir, project_path);
    let text = std::fs::read_to_string(file).ok()?;
    let saved: SavedLayout = toml::from_str(&text).ok()?;
    // A layout with no windows is not an answer, it is a file an older build
    // wrote after a capture that read nothing. Treating it as absent sends the
    // project back to its `[[layout]]`, which is what it had before the empty
    // file appeared. New ones cannot be written -- `write_layout` refuses --
    // so this is only ever about a file that is already there.
    (!saved.window.is_empty()).then_some(saved)
}

/// Write a project's layout, creating the directory the first time.
pub fn store(saved: &SavedLayout) -> anyhow::Result<std::path::PathBuf> {
    write_layout(saved, render(saved))
}

/// [`store`] under a named state directory.
#[cfg(test)]
pub fn store_in(
    state_dir: &std::path::Path,
    saved: &SavedLayout,
) -> anyhow::Result<std::path::PathBuf> {
    write_layout_in(state_dir, saved, render(saved))
}

/// [`store`], marking the panes a capture had to guess at.
pub fn store_rendered(
    saved: &SavedLayout,
    guessed: &[(usize, usize)],
) -> anyhow::Result<std::path::PathBuf> {
    write_layout(saved, render_with(saved, guessed))
}

/// The one place a layout file is written, so the refusal and the rename
/// cannot be skipped by adding another caller.
fn write_layout(saved: &SavedLayout, contents: String) -> anyhow::Result<std::path::PathBuf> {
    let dir = crate::server::state_dir()
        .ok_or_else(|| anyhow::anyhow!("no state directory: neither XDG_STATE_HOME nor HOME"))?;
    write_layout_in(&dir, saved, contents)
}

/// [`write_layout`] under a named state directory, so a test can point it at a
/// temporary one rather than at the environment.
fn write_layout_in(
    state_dir: &std::path::Path,
    saved: &SavedLayout,
    contents: String,
) -> anyhow::Result<std::path::PathBuf> {
    let file = path_in(state_dir, &saved.path);
    // A session always has at least one window, so a capture with none is a
    // capture that failed: tmux answered with nothing and every line was
    // skipped. Writing it would replace a good layout with a file that says
    // the project opens as a bare shell, and, because a saved layout wins over
    // the config, it would shadow the `[[layout]]` that used to cover this
    // project rather than fall back to it.
    if saved.window.is_empty() {
        anyhow::bail!(
            "refusing to save a layout with no windows for {}: nothing was captured,              so the file on disk is left alone",
            saved.path
        );
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_atomically(&file, &contents)?;
    Ok(file)
}

/// Write through a temporary file in the same directory and rename over the
/// target.
///
/// `std::fs::write` truncates first and writes second, so an interruption
/// between the two -- a full disk, a killed process, a container stopped
/// mid-save -- leaves a half-written file where a layout used to be. A rename
/// within one directory is atomic on every platform this runs on, so a reader
/// sees either the old file or the new one and never a piece of both.
///
/// The temporary file carries the process id, so two saves racing each other
/// cannot write to one scratch path. The loser of that race still leaves a
/// whole file behind, which is the property worth having.
fn write_atomically(file: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let tmp = file.with_extension(format!("toml.{}.tmp", std::process::id()));
    match std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, file)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Delete a project's saved layout, answering whether there was one.
pub fn forget(project_path: &str) -> anyhow::Result<bool> {
    let Some(file) = path_for(project_path) else {
        return Ok(false);
    };
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The windows a project opens with, and which file decided.
///
/// A saved layout wins over the config, because somebody pressed a key to make
/// it and the config is what they had before they did.
pub fn resolve(
    config: &Config,
    saved: Option<SavedLayout>,
    project_path: &str,
    home: &str,
) -> (Vec<LayoutWindow>, Source) {
    if let Some(s) = saved {
        let file = path_for(project_path).unwrap_or_default();
        return (s.window, Source::Saved(file));
    }
    match config.layout_for(project_path, home) {
        Some(l) => (l.window.clone(), Source::Named(l.name.clone())),
        None => (Vec::new(), Source::Nothing),
    }
}

// ── capture ──────────────────────────────────────────────────────────────────

/// One pane as tmux reported it, before any decision about what to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneReport {
    /// `#{window_index}`.
    pub window: u32,
    /// `#{pane_index}`.
    pub index: u32,
    /// `#{pane_current_path}`.
    pub cwd: String,
    /// `#{pane_current_command}`, the process running right now.
    pub current: String,
    /// `#{pane_start_command}`, empty for a pane split by hand.
    pub start: String,
}

/// Everything one capture produced: the layout, what it guessed at, and what it
/// could not read at all.
///
/// `skipped` is the difference between a lossy capture and a wrong one.
/// Guessing a command loses the arguments and the file says so; a line that
/// does not parse loses a whole pane or a whole window, silently, and the
/// layout that replaces the old one is simply smaller than the session it came
/// from. Carrying the dropped lines out of here is what lets the caller refuse
/// to write rather than report success over a hole.
#[derive(Debug, Clone, Default)]
pub struct Captured {
    /// The layout as it would be written.
    pub layout: SavedLayout,
    /// `(window, pane)` positions whose command came from the running process.
    pub guessed: Vec<(usize, usize)>,
    /// Lines tmux returned that could not be parsed, verbatim, so an error can
    /// quote the one that broke rather than describe it.
    pub skipped: Vec<String>,
}

/// What a capture decided about one pane's command, so the file can say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// The pane was created with this command, arguments and all.
    Exact,
    /// Taken from the running process, so any arguments are gone.
    Guessed,
    /// A shell prompt, which restores as a shell.
    Shell,
}

/// Split a `#{...}` line on tabs into exactly `n` fields, or nothing.
fn fields(line: &str, n: usize) -> Option<Vec<&str>> {
    let parts: Vec<&str> = line.splitn(n, '\t').collect();
    (parts.len() == n).then_some(parts)
}

/// Parse `list-panes -F '#{window_index}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_start_command}'`.
pub fn parse_panes(text: &str) -> Vec<PaneReport> {
    parse_panes_reporting(text).0
}

/// [`parse_panes`], also answering with the lines it could not read.
pub fn parse_panes_reporting(text: &str) -> (Vec<PaneReport>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match parse_pane(line) {
            Some(p) => out.push(p),
            None => skipped.push(line.to_string()),
        }
    }
    out.sort_by_key(|p| (p.window, p.index));
    (out, skipped)
}

fn parse_pane(line: &str) -> Option<PaneReport> {
    let f = fields(line, 5)?;
    Some(PaneReport {
        window: f[0].parse().ok()?,
        index: f[1].parse().ok()?,
        cwd: f[2].to_string(),
        current: f[3].to_string(),
        start: f[4].to_string(),
    })
}

/// What to restore in a pane, and how sure the capture is about it.
///
/// `pane_start_command` is the only exact answer and it is empty for every pane
/// somebody split by hand. Falling back to the running process loses the
/// arguments, and falling back to it when the running process is the login
/// shell would write a command that opens a shell inside the shell tmux already
/// started, so that case becomes no command at all.
pub fn command_for(pane: &PaneReport, shell: &str) -> (String, Confidence) {
    let shell_name = shell.rsplit('/').next().unwrap_or(shell);
    let start = pane.start.trim();
    if !start.is_empty() && start != shell && start != shell_name {
        return (start.to_string(), Confidence::Exact);
    }
    let current = pane.current.trim();
    if current.is_empty() || current == shell_name || current == shell {
        return (String::new(), Confidence::Shell);
    }
    (current.to_string(), Confidence::Guessed)
}

/// Everything `capture` needs that came from tmux.
#[derive(Debug, Clone, Copy)]
pub struct Capture<'a> {
    /// `list-windows -F '#{window_index}\t#{window_name}\t#{window_width}\t#{window_height}\t#{window_layout}'`.
    pub windows: &'a str,
    /// `list-panes -s -F` in the format [`parse_panes`] expects.
    pub panes: &'a str,
    /// `#{session_path}`, which is the project this belongs to.
    pub project: &'a str,
    /// `project` with every symlink resolved.
    ///
    /// macOS makes this necessary rather than tidy: `/var` is a symlink to
    /// `/private/var`, tmux reports a session's path unresolved and a pane's
    /// path resolved, so without both spellings every pane in a project under
    /// `/var` stores a `cwd` that is the project directory written the other
    /// way.
    pub project_real: &'a str,
    /// `$SHELL`, so a pane at a prompt is recognised as one.
    pub shell: &'a str,
    /// Home, so a pane under it stores a `~` and survives a different machine.
    pub home: &'a str,
    /// When this happened, as [`crate::tasks::format_unix`] writes it.
    pub at: &'a str,
    /// Whether to record what each pane was running.
    pub with_commands: bool,
}

/// Turn what tmux reported into a layout, plus the panes it had to guess at.
///
/// The guesses come back separately rather than being flagged inside the
/// layout, because the layout is a config type that a person may also write by
/// hand and it should not grow a field that only a capture ever sets.
pub fn capture(c: &Capture) -> Captured {
    let (panes, mut skipped) = parse_panes_reporting(c.panes);
    let mut layout = SavedLayout {
        path: c.project.to_string(),
        captured_at: c.at.to_string(),
        ..Default::default()
    };
    let mut guessed = Vec::new();

    for line in c.windows.lines().filter(|l| !l.trim().is_empty()) {
        let Some(f) = fields(line, 5) else {
            skipped.push(line.to_string());
            continue;
        };
        let Ok(index) = f[0].parse::<u32>() else {
            skipped.push(line.to_string());
            continue;
        };
        layout.width = layout.width.max(f[2].parse().unwrap_or(0));
        layout.height = layout.height.max(f[3].parse().unwrap_or(0));

        let mine: Vec<&PaneReport> = panes.iter().filter(|p| p.window == index).collect();
        let w = layout.window.len();
        let mut window = LayoutWindow {
            name: f[1].to_string(),
            command: String::new(),
            hold_name: true,
            layout: (mine.len() > 1).then(|| f[4].to_string()),
            main_size: None,
            pane: Vec::new(),
        };

        for (i, p) in mine.iter().enumerate() {
            let (command, how) = if c.with_commands {
                command_for(p, c.shell)
            } else {
                (String::new(), Confidence::Shell)
            };
            if how == Confidence::Guessed {
                guessed.push((w, i));
            }
            window.pane.push(LayoutPane {
                command,
                cwd: (p.cwd != c.project && p.cwd != c.project_real)
                    .then(|| short_dir(&p.cwd, c.home)),
                focus: false,
            });
        }

        // One pane is a window, not a window with a pane table, and writing it
        // the short way keeps a captured file readable next to a hand-written
        // one.
        if window.pane.len() <= 1 {
            let only = window.pane.pop();
            window.command = only.as_ref().map(|p| p.command.clone()).unwrap_or_default();
            window.layout = None;
            if let Some(cwd) = only.and_then(|p| p.cwd) {
                // A single pane cannot carry a cwd in the short form, so it
                // keeps its table rather than losing where it started.
                window.pane.push(LayoutPane {
                    command: std::mem::take(&mut window.command),
                    cwd: Some(cwd),
                    focus: false,
                });
            }
        }
        layout.window.push(window);
    }

    Captured {
        layout,
        guessed,
        skipped,
    }
}

/// `~/x` for a directory under home, unchanged otherwise.
fn short_dir(dir: &str, home: &str) -> String {
    match dir.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => dir.to_string(),
    }
}

// ── writing the file ─────────────────────────────────────────────────────────

/// A TOML string literal, with the two characters that need escaping handled.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The file a capture writes.
///
/// Written by hand rather than by serde so the guesses can carry a comment.
/// The person who reads this file needs to know which lines the tool invented,
/// and a serialiser has nowhere to put that.
pub fn render(saved: &SavedLayout) -> String {
    render_with(saved, &[])
}

/// [`render`], marking the panes a capture had to guess at.
pub fn render_with(saved: &SavedLayout, guessed: &[(usize, usize)]) -> String {
    let mut out = String::new();
    out.push_str("# Written by tmux-companion. Edit it freely: nothing rewrites this\n");
    out.push_str("# file until you press the save or the close binding again, and\n");
    out.push_str("# `tmux-companion project forget` deletes it.\n");
    out.push_str(&format!("path = {}\n", quote(&saved.path)));
    if !saved.captured_at.is_empty() {
        out.push_str(&format!("captured_at = {}\n", quote(&saved.captured_at)));
    }
    if saved.width > 0 && saved.height > 0 {
        out.push_str("\n# The window size this was captured at. A tmux layout string carries\n");
        out.push_str("# absolute cell sizes, so a much smaller screen falls back to tiled\n");
        out.push_str("# rather than restoring panes below their minimum size.\n");
        out.push_str(&format!("width = {}\n", saved.width));
        out.push_str(&format!("height = {}\n", saved.height));
    }

    for (w, window) in saved.window.iter().enumerate() {
        out.push_str("\n[[window]]\n");
        out.push_str(&format!("name = {}\n", quote(&window.name)));
        if !window.command.is_empty() {
            let guess = guessed.contains(&(w, 0)) && window.pane.is_empty();
            if guess {
                out.push_str(
                    "# taken from the running process, so any arguments it had are gone\n",
                );
            }
            out.push_str(&format!("command = {}\n", quote(&window.command)));
        }
        if !window.hold_name {
            out.push_str("hold_name = false\n");
        }
        if let Some(l) = &window.layout {
            out.push_str(&format!("layout = {}\n", quote(l)));
        }
        if let Some(s) = &window.main_size {
            out.push_str(&format!("main_size = {}\n", quote(s)));
        }
        for (i, pane) in window.pane.iter().enumerate() {
            out.push_str("\n  [[window.pane]]\n");
            if guessed.contains(&(w, i)) {
                out.push_str(
                    "  # taken from the running process, so any arguments it had are gone\n",
                );
            }
            if !pane.command.is_empty() {
                out.push_str(&format!("  command = {}\n", quote(&pane.command)));
            }
            if let Some(cwd) = &pane.cwd {
                out.push_str(&format!("  cwd = {}\n", quote(cwd)));
            }
            if pane.focus {
                out.push_str("  focus = true\n");
            }
        }
    }
    out
}

/// What `project show` prints: the layout in force and where it came from.
pub fn describe(
    project_path: &str,
    windows: &[LayoutWindow],
    source: &Source,
    home: &str,
) -> String {
    let panes: usize = windows.iter().map(|w| w.pane_count()).sum();
    let mut out = format!("{}\n", crate::project::short_path(project_path, home));
    match source {
        Source::Saved(file) => {
            out.push_str(&format!("  layout: saved  ({})\n", file.display()));
        }
        Source::Named(name) => {
            out.push_str(&format!("  layout: [[layout]] name = \"{name}\"\n"));
        }
        Source::Nothing => {
            out.push_str("  layout: none, so a plain shell\n");
        }
    }
    out.push_str(&format!(
        "  {} window{}, {} pane{}\n",
        windows.len(),
        if windows.len() == 1 { "" } else { "s" },
        panes,
        if panes == 1 { "" } else { "s" },
    ));
    for w in windows {
        out.push_str(&format!("    {} ({})\n", w.name, w.pane_count()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(window: u32, index: u32, cwd: &str, current: &str, start: &str) -> PaneReport {
        PaneReport {
            window,
            index,
            cwd: cwd.to_string(),
            current: current.to_string(),
            start: start.to_string(),
        }
    }

    fn cap<'a>(windows: &'a str, panes: &'a str) -> Capture<'a> {
        Capture {
            windows,
            panes,
            project: "/w/proj",
            project_real: "/w/proj",
            shell: "/bin/zsh",
            home: "/home/me",
            at: "2026-09-22 10:00:00 UTC",
            with_commands: true,
        }
    }

    // ── file names ───────────────────────────────────────────────────────────

    #[test]
    fn two_projects_with_the_same_basename_get_different_files() {
        // Flattening the separator would put ~/a/api and ~/b/api in one file
        // and the second save would silently eat the first.
        assert_ne!(file_name("/home/me/a/api"), file_name("/home/me/b/api"));
    }

    #[test]
    fn a_percent_in_a_directory_name_cannot_forge_a_separator() {
        assert_ne!(file_name("/a%2Fb"), file_name("/a/b"));
    }

    #[test]
    fn the_file_name_stays_readable() {
        assert_eq!(file_name("/w/proj"), "%2Fw%2Fproj.toml");
    }

    #[test]
    fn the_path_goes_under_a_projects_directory() {
        let p = path_in(std::path::Path::new("/state"), "/w/proj");
        assert_eq!(
            p,
            std::path::PathBuf::from("/state/projects/%2Fw%2Fproj.toml")
        );
    }

    // ── what a pane was running ──────────────────────────────────────────────

    #[test]
    fn a_start_command_wins_because_it_still_has_its_arguments() {
        let p = report(0, 0, "/w/proj", "nvim", "nvim src/config.rs");
        assert_eq!(
            command_for(&p, "/bin/zsh"),
            ("nvim src/config.rs".to_string(), Confidence::Exact)
        );
    }

    #[test]
    fn a_pane_at_a_prompt_records_no_command_at_all() {
        // Writing "zsh" back would restore a shell inside the shell tmux
        // already started in that pane.
        let p = report(0, 0, "/w/proj", "zsh", "");
        assert_eq!(
            command_for(&p, "/bin/zsh"),
            (String::new(), Confidence::Shell)
        );
        let full = report(0, 0, "/w/proj", "/bin/zsh", "");
        assert_eq!(
            command_for(&full, "/bin/zsh"),
            (String::new(), Confidence::Shell)
        );
    }

    #[test]
    fn a_pane_whose_start_command_was_the_shell_is_still_a_prompt() {
        let p = report(0, 0, "/w/proj", "zsh", "/bin/zsh");
        assert_eq!(
            command_for(&p, "/bin/zsh"),
            (String::new(), Confidence::Shell)
        );
    }

    #[test]
    fn falling_back_to_the_running_process_is_marked_as_a_guess() {
        let p = report(0, 0, "/w/proj", "nvim", "");
        assert_eq!(
            command_for(&p, "/bin/zsh"),
            ("nvim".to_string(), Confidence::Guessed)
        );
    }

    #[test]
    fn an_empty_pane_report_is_a_shell_rather_than_a_blank_command() {
        let p = report(0, 0, "/w/proj", "", "");
        assert_eq!(
            command_for(&p, "/bin/zsh"),
            (String::new(), Confidence::Shell)
        );
    }

    // ── parsing ──────────────────────────────────────────────────────────────

    #[test]
    fn panes_come_back_in_window_and_index_order() {
        let text = "1\t1\t/w\tzsh\t\n0\t1\t/w\tnvim\t\n0\t0\t/w\tzsh\t\n";
        let got: Vec<(u32, u32)> = parse_panes(text)
            .iter()
            .map(|p| (p.window, p.index))
            .collect();
        assert_eq!(got, vec![(0, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn a_start_command_containing_a_tab_survives_because_it_is_the_last_field() {
        let text = "0\t0\t/w\tsh\tsh -c 'a\tb'\n";
        assert_eq!(parse_panes(text)[0].start, "sh -c 'a\tb'");
    }

    #[test]
    fn a_short_line_is_skipped_rather_than_panicking() {
        assert!(parse_panes("0\t0\t/w\n").is_empty());
    }

    // ── capture ──────────────────────────────────────────────────────────────

    #[test]
    fn a_one_pane_window_is_captured_in_the_short_form() {
        // A captured file sits next to a hand-written one, so a window holding
        // one pane should read the way somebody would have typed it.
        let c = capture(&cap(
            "0\tedit\t80\t24\tabcd,80x24,0,0,0\n",
            "0\t0\t/w/proj\tnvim\tnvim\n",
        ));
        assert_eq!(c.layout.window.len(), 1);
        assert_eq!(c.layout.window[0].command, "nvim");
        assert!(c.layout.window[0].pane.is_empty());
        assert_eq!(
            c.layout.window[0].layout, None,
            "one pane needs no layout string"
        );
    }

    #[test]
    fn a_multi_pane_window_keeps_the_raw_layout_string() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tbb62,80x24,0,0{40x24,0,0,1,39x24,41,0,2}\n",
            "0\t0\t/w/proj\tnvim\tnvim\n0\t1\t/w/proj\tzsh\t\n",
        ));
        assert_eq!(
            c.layout.window[0].layout.as_deref(),
            Some("bb62,80x24,0,0{40x24,0,0,1,39x24,41,0,2}")
        );
        assert_eq!(c.layout.window[0].pane.len(), 2);
        assert_eq!(c.layout.window[0].pane[0].command, "nvim");
        assert_eq!(c.layout.window[0].pane[1].command, "");
    }

    #[test]
    fn a_pane_in_the_project_directory_stores_no_cwd() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tx\n",
            "0\t0\t/w/proj\tnvim\tnvim\n0\t1\t/home/me/src\tzsh\t\n",
        ));
        assert_eq!(c.layout.window[0].pane[0].cwd, None);
        assert_eq!(c.layout.window[0].pane[1].cwd.as_deref(), Some("~/src"));
    }

    #[test]
    fn a_pane_under_the_resolved_project_path_stores_no_cwd_either() {
        // macOS: the session reports /var/..., the pane reports /private/var/...
        // and they are the same directory.
        let mut c = cap("0\tedit\t80\t24\tx\n", "0\t0\t/private/w/proj\tzsh\t\n");
        c.project_real = "/private/w/proj";
        let c = capture(&c);
        assert_eq!(
            c.layout.window[0].pane.len(),
            0,
            "the short form, with no cwd"
        );
    }

    #[test]
    fn a_single_pane_that_wandered_keeps_its_table_rather_than_losing_the_cwd() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tx\n",
            "0\t0\t/elsewhere\tnvim\tnvim\n",
        ));
        assert_eq!(c.layout.window[0].pane.len(), 1);
        assert_eq!(
            c.layout.window[0].pane[0].cwd.as_deref(),
            Some("/elsewhere")
        );
        assert_eq!(
            c.layout.window[0].command, "",
            "the command moved into the pane"
        );
    }

    #[test]
    fn the_guesses_come_back_with_their_window_and_pane() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tx\n1\tai\t80\t24\tx\n",
            "0\t0\t/w/proj\tnvim\t\n1\t0\t/w/proj\tclaude\tclaude --resume\n",
        ));
        assert_eq!(
            c.guessed,
            vec![(0, 0)],
            "only the one with no start command"
        );
    }

    #[test]
    fn no_commands_records_the_shape_and_nothing_that_was_running() {
        let mut c = cap("0\tedit\t80\t24\tx\n", "0\t0\t/w/proj\tnvim\tnvim\n");
        c.with_commands = false;
        let c = capture(&c);
        assert_eq!(c.layout.window[0].command, "");
        assert!(
            c.guessed.is_empty(),
            "nothing was c.guessed because nothing was asked"
        );
    }

    #[test]
    fn the_size_is_the_largest_window_seen() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tx\n1\tai\t272\t67\tx\n",
            "0\t0\t/w/proj\tzsh\t\n1\t0\t/w/proj\tzsh\t\n",
        ));
        assert_eq!((c.layout.width, c.layout.height), (272, 67));
    }

    #[test]
    fn capture_records_the_project_and_the_time() {
        let c = capture(&cap("0\tedit\t80\t24\tx\n", "0\t0\t/w/proj\tzsh\t\n"));
        assert_eq!(c.layout.path, "/w/proj");
        assert_eq!(c.layout.captured_at, "2026-09-22 10:00:00 UTC");
    }

    // ── the file ─────────────────────────────────────────────────────────────

    #[test]
    fn what_is_written_parses_back_to_what_was_captured() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tbb62,80x24,0,0{40x24,0,0,1,39x24,41,0,2}\n1\tai\t80\t24\tx\n",
            "0\t0\t/w/proj\tnvim\tnvim src/config.rs\n0\t1\t/home/me/src\tzsh\t\n1\t0\t/w/proj\tclaude\tclaude\n",
        ));
        let back: SavedLayout = toml::from_str(&render(&c.layout)).expect("round trip");
        assert_eq!(back, c.layout);
    }

    #[test]
    fn a_guess_carries_a_comment_saying_so() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tx\n",
            "0\t0\t/w/proj\tnvim\t\n0\t1\t/w/proj\tzsh\t\n",
        ));
        let text = render_with(&c.layout, &c.guessed);
        let line = text
            .lines()
            .position(|l| l.contains("arguments it had are gone"))
            .expect("a comment");
        let command = text.lines().position(|l| l.contains("nvim")).unwrap();
        assert!(
            line < command,
            "the comment sits above the line it is about:\n{text}"
        );
        // And it still parses, which a comment in the wrong place would break.
        assert!(toml::from_str::<SavedLayout>(&text).is_ok(), "{text}");
    }

    #[test]
    fn a_quote_in_a_command_is_escaped_rather_than_ending_the_string() {
        let mut l = SavedLayout {
            path: "/w/proj".to_string(),
            ..Default::default()
        };
        l.window.push(LayoutWindow {
            name: "edit".to_string(),
            command: r#"sh -c "echo \a""#.to_string(),
            hold_name: true,
            layout: None,
            main_size: None,
            pane: Vec::new(),
        });
        let back: SavedLayout = toml::from_str(&render(&l)).expect("round trip");
        assert_eq!(back.window[0].command, r#"sh -c "echo \a""#);
    }

    #[test]
    fn the_header_tells_the_reader_nothing_will_overwrite_it_behind_their_back() {
        let l = SavedLayout {
            path: "/w/proj".to_string(),
            ..Default::default()
        };
        let text = render(&l);
        assert!(text.contains("project forget"), "{text}");
        assert!(text.starts_with('#'), "{text}");
    }

    // ── resolution ───────────────────────────────────────────────────────────

    #[test]
    fn a_saved_layout_wins_over_the_config() {
        // Somebody pressed a key to make the saved one, and the config is what
        // they had before they did.
        let c = Config::default();
        let saved = SavedLayout {
            path: "/w/proj".to_string(),
            window: vec![LayoutWindow {
                name: "one".to_string(),
                command: String::new(),
                hold_name: true,
                layout: None,
                main_size: None,
                pane: Vec::new(),
            }],
            ..Default::default()
        };
        let (windows, source) = resolve(&c, Some(saved), "/w/proj", "/home/me");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "one");
        assert!(matches!(source, Source::Saved(_)), "{source:?}");
    }

    #[test]
    fn with_nothing_saved_the_config_decides_and_says_which_layout() {
        let c = Config::default();
        let (windows, source) = resolve(&c, None, "/w/proj", "/home/me");
        assert_eq!(windows.len(), 2);
        assert_eq!(source, Source::Named("default".to_string()));
    }

    #[test]
    fn a_config_naming_a_layout_nothing_defines_resolves_to_nothing() {
        let c = crate::config::parse(
            "[project]\nlayout = \"nope\"\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap();
        let (windows, source) = resolve(&c, None, "/w/proj", "/home/me");
        assert!(windows.is_empty());
        assert_eq!(source, Source::Nothing);
    }

    #[test]
    fn describe_names_the_file_that_won() {
        let c = Config::default();
        let (windows, source) = resolve(&c, None, "/w/proj", "/home/me");
        let text = describe("/w/proj", &windows, &source, "/home/me");
        assert!(text.contains("[[layout]] name = \"default\""), "{text}");
        assert!(text.contains("2 windows, 2 panes"), "{text}");
        assert!(text.contains("edit (1)"), "{text}");
    }

    #[test]
    fn describe_counts_panes_and_not_windows() {
        let windows = vec![LayoutWindow {
            name: "edit".to_string(),
            command: String::new(),
            hold_name: true,
            layout: None,
            main_size: None,
            pane: vec![
                LayoutPane::default(),
                LayoutPane::default(),
                LayoutPane::default(),
            ],
        }];
        let text = describe("/w/proj", &windows, &Source::Nothing, "/home/me");
        assert!(text.contains("1 window, 3 panes"), "{text}");
    }

    #[test]
    fn describe_says_plain_shell_when_nothing_matched() {
        let text = describe("/w/proj", &[], &Source::Nothing, "/home/me");
        assert!(text.contains("plain shell"), "{text}");
        assert!(text.contains("0 windows, 0 panes"), "{text}");
    }

    // ── storing ──────────────────────────────────────────────────────────────

    #[test]
    fn store_and_load_go_through_a_real_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = path_in(dir.path(), "/w/proj");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let l = SavedLayout {
            path: "/w/proj".to_string(),
            window: vec![LayoutWindow {
                name: "edit".to_string(),
                command: "nvim".to_string(),
                hold_name: true,
                layout: None,
                main_size: None,
                pane: Vec::new(),
            }],
            ..Default::default()
        };
        std::fs::write(&file, render(&l)).unwrap();
        let back: SavedLayout = toml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(back, l);
    }

    // ── all or nothing ───────────────────────────────────────────────────────

    #[test]
    fn a_pane_line_that_does_not_parse_is_reported_rather_than_dropped() {
        // Restore is a deliberate act, so a save is held to the same bargain.
        // This used to vanish: the pane was filtered out, the layout came back
        // one pane short, and `project save` said it had saved the session.
        let c = capture(&cap(
            "0\tedit\t80\t24\tbb62,80x24,0,0{40x24,0,0,1,39x24,41,0,2}\n",
            "0\t0\t/w/proj\tnvim\t\nnot a pane line at all\n0\t1\t/w/proj\tzsh\t\n",
        ));
        assert_eq!(c.skipped, vec!["not a pane line at all".to_string()]);
    }

    #[test]
    fn a_window_line_that_does_not_parse_is_reported_too() {
        let c = capture(&cap(
            "0\tedit\t80\t24\tx\nrubbish\n",
            "0\t0\t/w/proj\tzsh\t\n",
        ));
        assert_eq!(c.skipped, vec!["rubbish".to_string()]);
    }

    #[test]
    fn a_window_index_that_is_not_a_number_is_reported() {
        let c = capture(&cap("x\tedit\t80\t24\tx\n", "0\t0\t/w/proj\tzsh\t\n"));
        assert_eq!(c.skipped.len(), 1);
        assert!(c.layout.window.is_empty());
    }

    #[test]
    fn a_clean_capture_reports_nothing_skipped() {
        let c = capture(&cap("0\tedit\t80\t24\tx\n", "0\t0\t/w/proj\tzsh\t\n"));
        assert!(c.skipped.is_empty(), "{:?}", c.skipped);
    }

    #[test]
    fn an_empty_capture_is_refused_rather_than_written() {
        // The failure this exists for: tmux answers with nothing, every line
        // is skipped, and a layout with no windows replaces a good one. The
        // command then prints "saved 0 windows" and exits 0.
        let t = tempfile::tempdir().unwrap();
        let empty = SavedLayout {
            path: "/w/proj".to_string(),
            ..Default::default()
        };
        let err = store_in(t.path(), &empty).expect_err("an empty layout must be refused");
        assert!(
            err.to_string().contains("no windows"),
            "the error says why: {err}"
        );
    }

    #[test]
    fn a_refused_save_leaves_the_file_that_was_there() {
        let t = tempfile::tempdir().unwrap();
        let good = SavedLayout {
            path: "/w/keepme".to_string(),
            window: vec![LayoutWindow {
                name: "edit".to_string(),
                command: "nvim".to_string(),
                hold_name: true,
                layout: None,
                main_size: None,
                pane: Vec::new(),
            }],
            ..Default::default()
        };
        let file = store_in(t.path(), &good).expect("the good one is written");
        let before = std::fs::read_to_string(&file).unwrap();

        let empty = SavedLayout {
            path: "/w/keepme".to_string(),
            ..Default::default()
        };
        assert!(store_in(t.path(), &empty).is_err());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            before,
            "the layout on disk is untouched by a refused save"
        );
    }

    #[test]
    fn an_empty_file_already_on_disk_reads_as_no_layout() {
        // Written by a build from before the refusal existed. Treating it as a
        // layout means the project opens as a bare shell with its [[layout]]
        // shadowed rather than used.
        let t = tempfile::tempdir().unwrap();
        let file = path_in(t.path(), "/w/empty");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "path = \"/w/empty\"\ncaptured_at = \"now\"\n").unwrap();
        assert!(load_in(t.path(), "/w/empty").is_none());
    }

    #[test]
    fn writing_leaves_no_temporary_file_behind() {
        // The rename is the point: a reader sees the old file or the new one.
        // A leftover scratch file would mean the rename never happened.
        let t = tempfile::tempdir().unwrap();
        let l = SavedLayout {
            path: "/w/tmpcheck".to_string(),
            window: vec![LayoutWindow {
                name: "edit".to_string(),
                command: String::new(),
                hold_name: true,
                layout: None,
                main_size: None,
                pane: Vec::new(),
            }],
            ..Default::default()
        };
        let file = store_in(t.path(), &l).unwrap();
        let dir = file.parent().unwrap();
        let strays: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(strays.is_empty(), "left behind: {strays:?}");
    }
}
