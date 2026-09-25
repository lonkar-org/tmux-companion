//! Turning three tmux listings into one snapshot.
//!
//! Everything here is a pure function over text tmux already printed, so the
//! whole module tests against a recorded server rather than a running one. The
//! fixtures under `tests/fixtures/server-*.tsv` are this laptop's own seven
//! sessions, taken on 2026-09-25 with the home directory rewritten, and they
//! are the shapes these parsers have to survive.
//!
//! Three listings rather than one. tmux has no format that reports a pane and
//! its window's layout in the same row, and asking per session would be one
//! process per session where this is three for the whole server.
//!
//! The confidence grading is [`crate::saved::command_for`], not a copy of it.
//! A pane's line here carries four fields a project capture's line does not, so
//! the two listings cannot share a line parser, but the decision about what a
//! pane should restore with has one implementation and this calls it.

use crate::saved::{Confidence, PaneReport, command_for};

use super::{Header, Pane, Session, Snapshot, Window};

/// `list-sessions -F` in the shape [`parse_sessions`] expects.
pub const SESSION_FORMAT: &str = "#{session_name}\t#{session_path}\t#{session_attached}";

/// `list-windows -a -F` in the shape [`parse_windows`] expects.
pub const WINDOW_FORMAT: &str = "#{session_name}\t#{window_index}\t#{window_name}\t#{window_active}\t#{window_zoomed_flag}\t#{window_layout}";

/// `list-panes -a -F` in the shape [`parse_panes`] expects.
///
/// A superset of what `[notify]` and `[window_names]` ask for, so one pair of
/// listings can feed all three once the daemon shares its poll.
pub const PANE_FORMAT: &str = "#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_start_command}\t#{pane_active}\t#{pane_pid}";

/// `ps` in the shape [`parse_process_table`] expects.
///
/// One call for the whole machine, not one per pane. It measured 7 ms of CPU
/// against 933 processes on this laptop, next to 130 ms already spent capturing
/// fourteen panes' history, so the scan the git segment is forbidden to do once
/// a second is affordable once a snapshot.
pub const PS_ARGS: &[&str] = &["-axo", "ppid=,args="];

/// Split a line into exactly `n` tab-separated fields, or nothing.
///
/// Exactly, where [`crate::saved`] splits into at most `n` and lets the last
/// field keep any tabs after it. That works there because the free-form field
/// is last. Here the paths and commands sit in the middle, so a stray tab in a
/// directory name would shift every field after it and quietly write a pane
/// whose command is half a path. A line that does not split cleanly is one this
/// module refuses to read, which the caller turns into a refusal to write.
fn fields(line: &str, n: usize) -> Option<Vec<&str>> {
    let parts: Vec<&str> = line.split('\t').collect();
    (parts.len() == n).then_some(parts)
}

/// tmux's own spelling of a flag: `1` is true and everything else is not.
fn flag(field: &str) -> bool {
    field.trim() == "1"
}

/// One session as `list-sessions` reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReport {
    /// `#{session_name}`.
    pub name: String,
    /// `#{session_path}`.
    pub path: String,
    /// `#{session_attached}`, which decides what a restore lands on.
    pub attached: bool,
}

/// One window as `list-windows -a` reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowReport {
    /// `#{session_name}`, which is what ties this row to a session.
    pub session: String,
    /// `#{window_index}`.
    pub index: u32,
    /// `#{window_name}`.
    pub name: String,
    /// `#{window_active}`.
    pub active: bool,
    /// `#{window_zoomed_flag}`.
    pub zoomed: bool,
    /// `#{window_layout}`.
    pub layout: String,
}

/// One pane as `list-panes -a` reported it, with the session and the fields
/// [`crate::saved::PaneReport`] does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPaneReport {
    /// `#{session_name}`.
    pub session: String,
    /// Everything the confidence grading needs, in the shape it already takes.
    pub pane: PaneReport,
    /// `#{pane_active}`.
    pub active: bool,
    /// `#{pane_pid}`, the shell tmux started in this pane.
    ///
    /// Not stored in the snapshot. It is the key into the process table, and
    /// it means nothing once the server it came from has exited.
    pub pid: u32,
}

/// Every process's parent and argument vector, by parent pid.
///
/// tmux has no format for the arguments of the program running in a pane. It
/// reports `#{pane_current_command}`, which is a process name, and for an agent
/// that name is its version string: `2.1.281` rather than
/// `claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5`. The identity of the
/// conversation is in the arguments, so without this the snapshot records a
/// number nothing can match and the restore has nothing to re-run.
///
/// Keyed by parent because the pane's own pid is the shell, and what we want is
/// the program it is running. A pane sitting at a prompt has no child and
/// answers nothing, which is the right answer for a pane at a prompt.
pub fn parse_process_table(text: &str) -> std::collections::HashMap<u32, String> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some((ppid, args)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(ppid) = ppid.parse::<u32>() else {
            continue;
        };
        let args = args.trim();
        if args.is_empty() {
            continue;
        }
        // First child wins. A shell with two children is a pipeline, and its
        // first process is the one somebody typed.
        out.entry(ppid).or_insert_with(|| args.to_string());
    }
    out
}

/// What a pane should restore with, given the process table as well.
///
/// The argument vector is the exact answer and beats both of the others: it is
/// what is running, arguments and all, where `pane_start_command` is what the
/// pane was opened with and `pane_current_command` has had its arguments cut
/// off. With no argument vector this is [`crate::saved::command_for`] and
/// nothing else, which is the answer on a machine where `ps` said nothing.
pub fn command_for_pane(
    pane: &PaneReport,
    full: Option<&str>,
    shell: &str,
    default_command: &str,
) -> (String, Confidence) {
    let fallback = command_for(pane, shell, default_command);
    let Some(full) = full.map(str::trim).filter(|f| !f.is_empty()) else {
        return fallback;
    };
    // A pane at a prompt whose shell has a child that is itself a shell is
    // still a pane at a prompt.
    if fallback.1 == Confidence::Shell && is_a_shell(full, shell) {
        return fallback;
    }
    (full.to_string(), Confidence::Exact)
}

/// Whether an argument vector is just a shell being a shell.
fn is_a_shell(full: &str, shell: &str) -> bool {
    let first = full.split_whitespace().next().unwrap_or(full);
    // A login shell prints with a leading dash, which is how `ps` reports the
    // one tmux started. `is_an_untouched_default_session` strips it for the
    // same reason.
    let name = first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .trim_start_matches('-');
    let shell_name = shell.rsplit('/').next().unwrap_or(shell);
    first == shell
        || name == shell_name
        || matches!(name, "zsh" | "bash" | "sh" | "fish" | "dash" | "ksh")
}

/// Parse `list-sessions -F` in [`SESSION_FORMAT`], with the lines it could not
/// read.
pub fn parse_sessions(text: &str) -> (Vec<SessionReport>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match fields(line, 3) {
            Some(f) => out.push(SessionReport {
                name: f[0].to_string(),
                path: f[1].to_string(),
                attached: flag(f[2]),
            }),
            None => skipped.push(line.to_string()),
        }
    }
    (out, skipped)
}

/// Parse `list-windows -a -F` in [`WINDOW_FORMAT`], with the lines it could not
/// read.
pub fn parse_windows(text: &str) -> (Vec<WindowReport>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let parsed = fields(line, 6).and_then(|f| {
            Some(WindowReport {
                session: f[0].to_string(),
                index: f[1].parse().ok()?,
                name: f[2].to_string(),
                active: flag(f[3]),
                zoomed: flag(f[4]),
                layout: f[5].to_string(),
            })
        });
        match parsed {
            Some(w) => out.push(w),
            None => skipped.push(line.to_string()),
        }
    }
    out.sort_by(|a, b| a.session.cmp(&b.session).then(a.index.cmp(&b.index)));
    (out, skipped)
}

/// Parse `list-panes -a -F` in [`PANE_FORMAT`], with the lines it could not
/// read.
pub fn parse_panes(text: &str) -> (Vec<ServerPaneReport>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let parsed = fields(line, 8).and_then(|f| {
            Some(ServerPaneReport {
                session: f[0].to_string(),
                pane: PaneReport {
                    window: f[1].parse().ok()?,
                    index: f[2].parse().ok()?,
                    cwd: f[3].to_string(),
                    current: f[4].to_string(),
                    start: f[5].to_string(),
                },
                active: flag(f[6]),
                pid: f[7].parse().ok()?,
            })
        });
        match parsed {
            Some(p) => out.push(p),
            None => skipped.push(line.to_string()),
        }
    }
    out.sort_by(|a, b| {
        a.session
            .cmp(&b.session)
            .then(a.pane.window.cmp(&b.pane.window))
            .then(a.pane.index.cmp(&b.pane.index))
    });
    (out, skipped)
}

/// Everything one capture of the server needs, as one struct so the signature
/// stays readable.
#[derive(Debug, Clone, Copy)]
pub struct Capture<'a> {
    /// `list-sessions -F` in [`SESSION_FORMAT`].
    pub sessions: &'a str,
    /// `list-windows -a -F` in [`WINDOW_FORMAT`].
    pub windows: &'a str,
    /// `list-panes -a -F` in [`PANE_FORMAT`].
    pub panes: &'a str,
    /// `ps` in [`PS_ARGS`], empty when it could not be run.
    pub processes: &'a str,
    /// `$SHELL`, so a pane at a prompt is recognised as one.
    pub shell: &'a str,
    /// `show-options -gv default-command`, for the same reason a project
    /// capture needs it: tmux reports it as the start command of every pane
    /// nobody gave a command to.
    pub default_command: &'a str,
    /// Session names never captured, from `[sessions] exclude`.
    pub exclude: &'a [String],
    /// When this happened, as [`crate::tasks::format_unix`] writes it.
    pub at: &'a str,
    /// What `tmux -V` said.
    pub tmux_version: &'a str,
    /// The build taking the capture.
    pub companion_version: &'a str,
    /// This machine.
    pub hostname: &'a str,
    /// Whether the capture is being taken on the way to a clean shutdown.
    pub clean: bool,
}

/// A snapshot, and what the capture was unsure of or could not read.
#[derive(Debug, Clone, Default)]
pub struct Captured {
    /// The snapshot as it would be written.
    pub snapshot: Snapshot,
    /// `(session, window, pane)` for every pane whose command came from the
    /// running process rather than from what it was started with.
    pub guessed: Vec<(String, u32, u32)>,
    /// Lines tmux returned that could not be read, verbatim.
    pub skipped: Vec<String>,
    /// Sessions left out because `[sessions] exclude` named them.
    pub excluded: Vec<String>,
}

impl Captured {
    /// Why this capture must not be written, or `None` when it is whole.
    ///
    /// A skipped line loses a pane or a window silently, and the snapshot that
    /// replaces the last one is then smaller than the server it came from.
    /// Losing arguments to a guess is a lossy capture that says so in the file;
    /// losing a line is a wrong one, so it stops the write instead.
    pub fn refusal(&self) -> Option<String> {
        if let Some(first) = self.skipped.first() {
            return Some(format!(
                "{} line(s) of tmux output could not be read, so nothing was written. The first: {first}",
                self.skipped.len()
            ));
        }
        self.snapshot
            .session
            .is_empty()
            .then(|| "no sessions were captured, so nothing was written".to_string())
    }
}

/// Build a snapshot from three tmux listings.
///
/// Windows and panes are filed under the session each row names, and a row
/// naming a session that `list-sessions` did not report is dropped: tmux was
/// asked three times and a session that appeared between the calls has no path
/// and no place to be restored to.
pub fn capture(c: &Capture) -> Captured {
    let (sessions, mut skipped) = parse_sessions(c.sessions);
    let (windows, window_skips) = parse_windows(c.windows);
    let (panes, pane_skips) = parse_panes(c.panes);
    let processes = parse_process_table(c.processes);
    skipped.extend(window_skips);
    skipped.extend(pane_skips);

    let attached = sessions
        .iter()
        .find(|s| s.attached)
        .map(|s| s.name.clone())
        .unwrap_or_default();

    let mut excluded = Vec::new();
    let mut guessed = Vec::new();
    let mut out = Vec::new();

    for session in &sessions {
        if c.exclude.contains(&session.name) {
            excluded.push(session.name.clone());
            continue;
        }
        let mut built = Session {
            name: session.name.clone(),
            path: session.path.clone(),
            window: Vec::new(),
        };
        for window in windows.iter().filter(|w| w.session == session.name) {
            let mut panes_here = Vec::new();
            for report in panes
                .iter()
                .filter(|p| p.session == session.name && p.pane.window == window.index)
            {
                let (command, how) = command_for_pane(
                    &report.pane,
                    processes.get(&report.pid).map(String::as_str),
                    c.shell,
                    c.default_command,
                );
                if how == Confidence::Guessed {
                    guessed.push((session.name.clone(), window.index, report.pane.index));
                }
                panes_here.push(Pane {
                    index: report.pane.index,
                    cwd: report.pane.cwd.clone(),
                    command,
                    confidence: how,
                    active: report.active,
                });
            }
            built.window.push(Window {
                index: window.index,
                name: window.name.clone(),
                layout: window.layout.clone(),
                active: window.active,
                zoomed: window.zoomed,
                pane: panes_here,
            });
        }
        out.push(built);
    }

    Captured {
        snapshot: Snapshot {
            header: Header {
                format: super::FORMAT,
                captured_at: c.at.to_string(),
                clean: c.clean,
                tmux_version: c.tmux_version.to_string(),
                companion_version: c.companion_version.to_string(),
                hostname: c.hostname.to_string(),
                attached,
                imported_from: String::new(),
            },
            session: out,
        },
        guessed,
        skipped,
        excluded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSIONS: &str = include_str!("../../tests/fixtures/server-sessions.tsv");
    const WINDOWS: &str = include_str!("../../tests/fixtures/server-windows.tsv");
    const PANES: &str = include_str!("../../tests/fixtures/server-panes.tsv");
    const PROCESSES: &str = include_str!("../../tests/fixtures/server-processes.tsv");

    /// The laptop these fixtures came from: zsh behind the macOS wrapper that
    /// every pane reports as its start command.
    fn laptop(exclude: &'static [String]) -> Capture<'static> {
        Capture {
            sessions: SESSIONS,
            windows: WINDOWS,
            panes: PANES,
            processes: PROCESSES,
            shell: "/bin/zsh",
            default_command: "reattach-to-user-namespace -l /bin/zsh",
            exclude,
            at: "2026-09-25 09:23:16 UTC",
            tmux_version: "tmux 3.7c",
            companion_version: "0.2.0+1790261531",
            hostname: "laptop",
            clean: true,
        }
    }

    #[test]
    fn the_whole_server_comes_back_with_every_session_window_and_pane() {
        let got = capture(&laptop(&[]));
        assert!(got.refusal().is_none(), "{:?}", got.refusal());
        assert_eq!(got.snapshot.session.len(), 7);
        assert_eq!(got.snapshot.window_count(), 14);
        assert_eq!(got.snapshot.pane_count(), 14);
        assert!(got.skipped.is_empty(), "{:?}", got.skipped);
    }

    #[test]
    fn the_attached_session_is_the_one_tmux_flagged() {
        let got = capture(&laptop(&[]));
        assert_eq!(got.snapshot.header.attached, "tmux-companion");
        assert_eq!(got.snapshot.attach_target(), Some("tmux-companion"));
    }

    #[test]
    fn the_macos_shell_wrapper_never_becomes_a_pane_command() {
        // Every pane on this server reports the same start command, and it is
        // the setting, not a command anybody typed.
        let got = capture(&laptop(&[]));
        for session in &got.snapshot.session {
            for window in &session.window {
                for pane in &window.pane {
                    assert!(
                        !pane.command.contains("reattach-to-user-namespace"),
                        "{}:{} kept the wrapper",
                        session.name,
                        window.index
                    );
                }
            }
        }
    }

    #[test]
    fn an_agent_pane_keeps_the_conversation_it_was_in() {
        // The whole reason this feature exists. tmux reports the process name,
        // which for an agent is its version string, and the identity of the
        // conversation is in the arguments. Without the process table this pane
        // captures as `2.1.281` and nothing can restore it.
        let got = capture(&laptop(&[]));
        let ai = got
            .snapshot
            .session
            .iter()
            .find(|s| s.name == "icf-c_com")
            .and_then(|s| s.window.iter().find(|w| w.name == "ai"))
            .and_then(|w| w.pane.first())
            .expect("an ai window");
        assert_eq!(
            ai.command,
            "claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5"
        );
        assert_eq!(ai.confidence, Confidence::Exact);
    }

    #[test]
    fn without_a_process_table_a_pane_falls_back_to_the_process_name() {
        // A machine where `ps` answered nothing still captures, and says so by
        // grading what it got as a guess.
        let mut c = laptop(&[]);
        c.processes = "";
        let got = capture(&c);
        let ai = got
            .snapshot
            .session
            .iter()
            .find(|s| s.name == "icf-c_com")
            .and_then(|s| s.window.iter().find(|w| w.name == "ai"))
            .and_then(|w| w.pane.first())
            .expect("an ai window");
        assert_eq!(ai.command, "2.1.281");
        assert_eq!(ai.confidence, Confidence::Guessed);
    }

    #[test]
    fn the_process_table_keys_the_first_child_of_each_shell() {
        let table = parse_process_table("  61733 nvim\n61737 claude --resume abc\nnope\n7 \n");
        assert_eq!(table.get(&61733).map(String::as_str), Some("nvim"));
        assert_eq!(
            table.get(&61737).map(String::as_str),
            Some("claude --resume abc")
        );
        // A row with no arguments and a row that is not a number are both
        // skipped rather than stored as empty.
        assert_eq!(table.get(&7), None);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn a_pipeline_records_the_first_process_somebody_typed() {
        let table = parse_process_table("99 grep -n foo\n99 less\n");
        assert_eq!(table.get(&99).map(String::as_str), Some("grep -n foo"));
    }

    #[test]
    fn a_shell_under_a_shell_is_still_a_prompt() {
        // `ps` can report the login shell itself as a child on some setups.
        // Taking that as a command would restore a shell inside the shell tmux
        // already started, which is the bug command_for exists to avoid.
        let pane = PaneReport {
            window: 1,
            index: 1,
            cwd: "/w".to_string(),
            current: "zsh".to_string(),
            start: String::new(),
        };
        let (command, how) = command_for_pane(&pane, Some("-zsh"), "/bin/zsh", "");
        assert_eq!(command, "");
        assert_eq!(how, Confidence::Shell);
    }

    #[test]
    fn a_pane_at_a_prompt_carries_no_command_at_all() {
        let got = capture(&laptop(&[]));
        let scratch = got
            .snapshot
            .session
            .iter()
            .find(|s| s.name == "y")
            .expect("the scratch session");
        let pane = &scratch.window[0].pane[0];
        assert_eq!(pane.command, "");
        assert_eq!(pane.confidence, Confidence::Shell);
    }

    #[test]
    fn a_session_with_three_windows_keeps_all_three_in_index_order() {
        let got = capture(&laptop(&[]));
        let three = got
            .snapshot
            .session
            .iter()
            .find(|s| s.name == "lonkar_org")
            .expect("the three-window session");
        let indices: Vec<u32> = three.window.iter().map(|w| w.index).collect();
        assert_eq!(indices, vec![1, 2, 3]);
        // The third window sits in a different directory from its session,
        // which is the case a capture keyed on the session path would lose.
        assert_eq!(
            three.window[2].pane[0].cwd,
            "/Users/you/lonkar-org/elsonsind.com"
        );
    }

    #[test]
    fn the_layout_string_survives_verbatim() {
        let got = capture(&laptop(&[]));
        assert_eq!(
            got.snapshot.session[0].window[0].layout,
            "b644,170x52,0,0,7"
        );
    }

    #[test]
    fn nothing_is_a_guess_when_the_process_table_answered_for_every_pane() {
        // Thirteen panes are running something and `ps` named all thirteen, so
        // every command in this snapshot is the argument vector that was
        // running. The fourteenth is a prompt.
        let got = capture(&laptop(&[]));
        assert!(got.guessed.is_empty(), "{:?}", got.guessed);
    }

    #[test]
    fn a_pane_the_process_table_missed_is_reported_as_a_guess() {
        let thinner = PROCESSES
            .lines()
            .filter(|l| !l.starts_with("61737 "))
            .collect::<Vec<_>>()
            .join("\n");
        let mut c = laptop(&[]);
        c.processes = &thinner;
        let got = capture(&c);
        assert_eq!(got.guessed, vec![("icf-c_com".to_string(), 2, 1)]);
    }

    #[test]
    fn an_excluded_session_is_left_out_and_named() {
        let exclude: &'static [String] = Box::leak(Box::new(["y".to_string()]));
        let got = capture(&laptop(exclude));
        assert_eq!(got.snapshot.session.len(), 6);
        assert!(!got.snapshot.session.iter().any(|s| s.name == "y"));
        assert_eq!(got.excluded, vec!["y".to_string()]);
        assert!(got.refusal().is_none());
    }

    #[test]
    fn a_malformed_line_stops_the_write_rather_than_shrinking_the_snapshot() {
        let broken = format!("{PANES}icf-c_com\t1\tnot-a-number\t/w\tnvim\t\t1\t99\n");
        let mut c = laptop(&[]);
        c.panes = &broken;
        let got = capture(&c);

        assert_eq!(got.skipped.len(), 1);
        let refusal = got.refusal().expect("a refusal");
        assert!(refusal.contains("could not be read"), "{refusal}");
        assert!(refusal.contains("not-a-number"), "{refusal}");
    }

    #[test]
    fn a_tab_inside_a_directory_name_is_refused_rather_than_shifted() {
        // splitn would put the tail of the path into the next field and write a
        // pane whose command is half a directory. Exact splitting turns that
        // into a line nobody reads and a write nobody makes.
        let broken = "s\t1\t1\t/w/od\td\tnvim\t\t1\t99\n";
        let (panes, skipped) = parse_panes(broken);
        assert!(panes.is_empty());
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn an_empty_server_is_a_refusal_and_not_an_empty_file() {
        let mut c = laptop(&[]);
        c.sessions = "";
        c.windows = "";
        c.panes = "";
        c.processes = "";
        let got = capture(&c);
        assert!(got.skipped.is_empty());
        assert!(got.refusal().expect("a refusal").contains("no sessions"));
    }

    #[test]
    fn excluding_every_session_is_a_refusal_too() {
        let all: &'static [String] = Box::leak(Box::new([
            "icf-c_com".to_string(),
            "lekhani".to_string(),
            "lonkar_org".to_string(),
            "mysetup".to_string(),
            "tmux-companion".to_string(),
            "y".to_string(),
            "yogesh_lonkar_org".to_string(),
        ]));
        let got = capture(&laptop(all));
        assert!(got.refusal().is_some());
        assert_eq!(got.excluded.len(), 7);
    }

    #[test]
    fn a_window_naming_a_session_that_went_between_the_calls_is_dropped() {
        let extra = format!("{WINDOWS}ghost\t1\tedit\t1\t0\tb644,170x52,0,0,99\n");
        let mut c = laptop(&[]);
        c.windows = &extra;
        let got = capture(&c);
        assert_eq!(got.snapshot.session.len(), 7);
        assert_eq!(got.snapshot.window_count(), 14);
        assert!(got.refusal().is_none());
    }

    #[test]
    fn zoom_and_the_active_flags_come_through() {
        let text = "s\t1\tedit\t1\t1\tb644,170x52,0,0,7\n";
        let (windows, skipped) = parse_windows(text);
        assert!(skipped.is_empty());
        assert!(windows[0].active);
        assert!(windows[0].zoomed);
    }

    #[test]
    fn the_pane_format_is_a_superset_of_what_the_other_pollers_ask_for() {
        // The daemon is meant to make one pair of listings and hand the result
        // to notify, window_names and the snapshot timer. That only works while
        // this format carries what those two read.
        for field in [
            "#{session_name}",
            "#{pane_current_command}",
            "#{pane_active}",
        ] {
            assert!(PANE_FORMAT.contains(field), "{field} missing");
        }
        assert!(WINDOW_FORMAT.contains("#{window_name}"));
    }
}
