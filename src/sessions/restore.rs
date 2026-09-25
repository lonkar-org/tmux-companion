//! Rebuilding a server from a snapshot.
//!
//! The command list is a pure function, the way [`crate::project::session_commands`]
//! is, and for the same reason: building a session is a few dozen ordered calls
//! whose order is the entire feature, and the only way to test an order is to
//! look at it without a tmux server in the room. It is also what makes
//! `--dry-run` honest, since the listing it prints is the list that would run
//! rather than a second description of it.
//!
//! [`crate::project::session_commands`] itself cannot be reused here, which is
//! worth saying because reusing it was the plan. It builds from a config
//! layout, which has no window indices, no tmux layout string and no zoom, and
//! it targets windows by name. This laptop has a session holding two windows
//! both called `edit`, so a restore that targeted by name would build one of
//! them twice and leave the other empty. Everything here targets by index.

use crate::restore::{Decision, Planned};

use super::Snapshot;

/// Why a restore did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The server already holds sessions, and this would have landed on top.
    Live(Vec<String>),
    /// There was nothing in the snapshot to build.
    Empty,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Live(names) => write!(
                f,
                "{} session{} already running: {}\nnothing restored. --merge adds the ones that are missing, --only picks some",
                names.len(),
                if names.len() == 1 { "" } else { "s" },
                names.join(", ")
            ),
            Refusal::Empty => write!(f, "nothing in this snapshot to restore"),
        }
    }
}

/// The exit code a refusal earns, for a caller nobody is watching.
impl Refusal {
    /// `3` for a server that is already busy, `4` for nothing to do.
    pub fn code(&self) -> i32 {
        match self {
            Refusal::Live(_) => 3,
            Refusal::Empty => 4,
        }
    }
}

/// Everything the command builder needs.
#[derive(Debug, Clone, Copy)]
pub struct RestoreSpec<'a> {
    /// The snapshot to rebuild.
    pub snapshot: &'a Snapshot,
    /// What the restore table decided for each pane, from [`crate::restore::plan`].
    pub plan: &'a [Planned],
    /// Sessions already on this server, which are never touched.
    pub live: &'a [String],
    /// Only these sessions, when the list is not empty.
    pub only: &'a [String],
    /// Never these sessions.
    pub exclude: &'a [String],
    /// Add what is missing rather than refusing a server that has sessions.
    pub merge: bool,
    /// `$HOME`, for a pane whose directory is gone.
    pub home: &'a str,
    /// The server's `base-index`.
    pub base_index: u32,
    /// The server's `pane-base-index`.
    pub pane_base: u32,
    /// Directories that do not exist on this machine, so the caller can say so.
    pub missing: &'a [String],
}

/// One thing a restore does, in order.
///
/// A restore is not only tmux calls: before a pane is told to run something it
/// has to be ready to be told, and that is a wait rather than a command. Making
/// it a step rather than a side effect inside the runner keeps `--dry-run`
/// honest, since the listing then shows the waits too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Run `tmux` with these arguments.
    Tmux(Vec<String>),
    /// Wait for this pane's shell to draw a prompt before going on.
    WaitForPrompt(String),
}

impl Step {
    /// The tmux arguments, for a step that is a tmux call.
    pub fn tmux(&self) -> Option<&[String]> {
        match self {
            Step::Tmux(args) => Some(args),
            Step::WaitForPrompt(_) => None,
        }
    }
}

/// How long to wait for a shell to say it has drawn a prompt.
///
/// A shell that emits OSC 133 answers in milliseconds, so this is only ever the
/// ceiling for one that never will: a shell with no `shell-init` line in its rc
/// file, or one still loading a slow plugin. Past it the restore sends anyway,
/// which is what every version of this has always done.
pub const PROMPT_WAIT: std::time::Duration = std::time::Duration::from_millis(2_000);

/// How often to ask, while waiting.
pub const PROMPT_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Whether one line of `capture-pane -p -F` output is a prompt.
///
/// tmux writes the flags as the first space-separated field, `-` when a line
/// has none, so the test is unambiguous even for a line whose text begins with
/// a capital P.
pub fn is_a_prompt_line(line: &str) -> bool {
    line.split(' ')
        .next()
        .is_some_and(|flags| flags.contains('P'))
}

/// Whether a pane has drawn a prompt, from its captured flags.
pub fn has_drawn_a_prompt(capture: &str) -> bool {
    capture.lines().any(is_a_prompt_line)
}

/// What a restore would do, before it does any of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rebuild {
    /// The steps, in the order they have to run.
    pub commands: Vec<Step>,
    /// The sessions this would create.
    pub sessions: Vec<String>,
    /// Sessions in the snapshot that were skipped, and why, one line each.
    pub skipped: Vec<String>,
}

/// Which sessions a restore would build, after `--only`, `--exclude` and what
/// is already running.
///
/// A live session is never rebuilt and never modified. Under `--merge` it is
/// skipped and the rest are built; without it, one live session is enough to
/// refuse the whole restore, because a restore that half-landed on somebody's
/// working state is the failure worth being careful about.
pub fn wanted(spec: &RestoreSpec) -> Result<Vec<String>, Refusal> {
    let mut clashes = Vec::new();
    let mut out = Vec::new();
    for session in &spec.snapshot.session {
        let name = &session.name;
        if !spec.only.is_empty() && !spec.only.contains(name) {
            continue;
        }
        if spec.exclude.contains(name) {
            continue;
        }
        if spec.live.contains(name) {
            clashes.push(name.clone());
            continue;
        }
        out.push(name.clone());
    }
    if !clashes.is_empty() && !spec.merge {
        return Err(Refusal::Live(clashes));
    }
    // A server holding sessions this snapshot says nothing about is still a
    // server somebody is using, so plain `resurrect` refuses that too.
    if !spec.merge && !spec.live.is_empty() {
        return Err(Refusal::Live(spec.live.to_vec()));
    }
    if out.is_empty() {
        return Err(Refusal::Empty);
    }
    Ok(out)
}

/// One tmux invocation, owned.
fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

/// The tmux commands that rebuild a server, in the order they have to run.
///
/// Per session: the session and its windows, then the panes, then the geometry,
/// then the commands, then zoom, then what was selected. Commands last for the
/// reason `session_commands` gives, and geometry before them for the same one:
/// a full-screen program drawn at the pre-split size repaints itself, which
/// looks broken on every restore.
///
/// Selection last of all, because every `new-window` and `split-window` moves
/// it, so anything that ran afterwards would undo it.
pub fn rebuild(spec: &RestoreSpec) -> Result<Rebuild, Refusal> {
    let names = wanted(spec)?;
    let mut out = Rebuild {
        sessions: names.clone(),
        ..Rebuild::default()
    };

    for session in spec
        .snapshot
        .session
        .iter()
        .filter(|s| names.contains(&s.name))
    {
        let Some((first, rest)) = session.window.split_first() else {
            out.skipped
                .push(format!("{}: no windows in the snapshot", session.name));
            continue;
        };
        let target = |window: u32| format!("={}:{window}", session.name);
        // Each window opens where its own first pane was, not where its session
        // was. A session holding a window in a sibling checkout is ordinary --
        // this laptop has one -- and using the session path for every window
        // restored that window to the wrong repository while reporting success.
        let window_dir = |w: &super::Window| {
            let own = w.pane.first().map(|p| p.cwd.as_str()).unwrap_or_default();
            directory(if own.is_empty() { &session.path } else { own }, spec)
        };
        let path = window_dir(first);

        out.commands.push(Step::Tmux(args(&[
            "new-session",
            "-d",
            "-s",
            &session.name,
            "-c",
            &path,
            "-n",
            &first.name,
        ])));
        // tmux numbers the first window from `base-index`, which is whatever
        // this machine's config says rather than whatever the old one's did.
        //
        // The source is `^`, tmux's own name for the lowest-numbered window,
        // rather than the number we think that is. Reading `base-index` off a
        // server that does not exist yet answers nothing, which parsed as zero
        // and moved a window that was never there: `can't find window: 0`,
        // once per session, on the first restore this was ever run against.
        if first.index != spec.base_index {
            out.commands.push(Step::Tmux(args(&[
                "move-window",
                "-s",
                &format!("={}:^", session.name),
                "-t",
                &target(first.index),
            ])));
        }
        for window in rest {
            let here = window_dir(window);
            out.commands.push(Step::Tmux(args(&[
                "new-window",
                "-d",
                "-t",
                &target(window.index),
                "-c",
                &here,
                "-n",
                &window.name,
            ])));
        }

        for window in &session.window {
            let where_ = target(window.index);
            for pane in window.pane.iter().skip(1) {
                let cwd = directory(&pane.cwd, spec);
                out.commands.push(Step::Tmux(args(&[
                    "split-window",
                    "-t",
                    &where_,
                    "-c",
                    &cwd,
                ])));
            }
            // The saved layout string carries the real sizes, so this restores
            // the shape rather than an even split. It is applied after every
            // pane exists and before anything is told to run.
            if !window.layout.is_empty() && window.pane.len() > 1 {
                out.commands.push(Step::Tmux(args(&[
                    "select-layout",
                    "-t",
                    &where_,
                    &window.layout,
                ])));
            }
        }

        for window in &session.window {
            for pane in &window.pane {
                let decision = spec
                    .plan
                    .iter()
                    .find(|p| {
                        p.session == session.name
                            && p.window == window.index
                            && p.pane == pane.index
                    })
                    .map(|p| &p.decision);
                let Some(Decision::Run(command)) = decision else {
                    continue;
                };
                let at = format!("={}:{}.{}", session.name, window.index, pane.index);
                // The pane has to have drawn a prompt before it can be typed
                // into. Sending first is how a restore loses a command to a
                // shell that was still loading its rc file.
                out.commands.push(Step::WaitForPrompt(at.clone()));
                out.commands
                    .push(Step::Tmux(args(&["send-keys", "-t", &at, command, "C-m"])));
            }
        }

        for window in &session.window {
            if window.zoomed {
                out.commands.push(Step::Tmux(args(&[
                    "resize-pane",
                    "-Z",
                    "-t",
                    &target(window.index),
                ])));
            }
            if let Some(active) = window.pane.iter().find(|p| p.active) {
                let at = format!("={}:{}.{}", session.name, window.index, active.index);
                out.commands
                    .push(Step::Tmux(args(&["select-pane", "-t", &at])));
            }
        }
        if let Some(active) = session.window.iter().find(|w| w.active) {
            out.commands.push(Step::Tmux(args(&[
                "select-window",
                "-t",
                &target(active.index),
            ])));
        }
    }

    Ok(out)
}

/// A pane's directory, falling back to home when it is not on this machine.
///
/// These dotfiles get synced between machines and a snapshot names directories
/// that may exist on only one of them. Opening at home is a restore that worked
/// with one pane in the wrong place; failing is a restore that did not happen.
fn directory(dir: &str, spec: &RestoreSpec) -> String {
    if spec.missing.iter().any(|m| m == dir) {
        return spec.home.to_string();
    }
    dir.to_string()
}

/// Which of a snapshot's directories are not on this machine.
pub fn missing_directories(snap: &Snapshot, exists: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for session in &snap.session {
        for dir in std::iter::once(&session.path).chain(
            session
                .window
                .iter()
                .flat_map(|w| w.pane.iter().map(|p| &p.cwd)),
        ) {
            if !dir.is_empty() && !out.contains(dir) && !exists(dir) {
                out.push(dir.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::restore::plan;
    use crate::sessions::{Header, Pane, Session, Window};

    fn pane(index: u32, command: &str, active: bool) -> Pane {
        Pane {
            index,
            cwd: "/w/proj".to_string(),
            command: command.to_string(),
            confidence: crate::saved::Confidence::Exact,
            active,
        }
    }

    fn window(index: u32, name: &str, panes: Vec<Pane>) -> Window {
        Window {
            index,
            name: name.to_string(),
            layout: format!("bb62,170x52,0,0,{index}"),
            active: index == 1,
            zoomed: false,
            pane: panes,
        }
    }

    /// A session shaped like `lonkar_org`: two windows sharing one name.
    fn two_edits() -> Snapshot {
        Snapshot {
            header: Header::default(),
            session: vec![Session {
                name: "lonkar_org".to_string(),
                path: "/w/proj".to_string(),
                window: vec![
                    window(1, "edit", vec![pane(1, "nvim", true)]),
                    window(2, "ai", vec![pane(1, "claude --resume abc", true)]),
                    window(3, "edit", vec![pane(1, "nvim", true)]),
                ],
            }],
        }
    }

    fn spec<'a>(snap: &'a Snapshot, planned: &'a [Planned], live: &'a [String]) -> RestoreSpec<'a> {
        RestoreSpec {
            snapshot: snap,
            plan: planned,
            live,
            only: &[],
            exclude: &[],
            merge: false,
            home: "/Users/you",
            base_index: 1,
            pane_base: 1,
            missing: &[],
        }
    }

    fn flat(rebuild: &Rebuild) -> Vec<String> {
        rebuild
            .commands
            .iter()
            .filter_map(Step::tmux)
            .map(|c| c.join(" "))
            .collect()
    }

    #[test]
    fn a_prompt_line_is_read_from_the_flag_field_and_not_from_the_text() {
        assert!(is_a_prompt_line("P ~/lonkar-org/tmux-companion $ "));
        assert!(is_a_prompt_line("OP some line"));
        // The flag field is `-` when a line has none, so a line whose text
        // begins with a capital P is not a prompt.
        assert!(!is_a_prompt_line("- Path/to/thing"));
        assert!(!is_a_prompt_line("O Program output"));
        assert!(!is_a_prompt_line(""));
    }

    #[test]
    fn a_pane_that_has_drawn_a_prompt_says_so_and_an_empty_one_does_not() {
        assert!(has_drawn_a_prompt("O output\n- \nP ~ $ \n"));
        assert!(!has_drawn_a_prompt("- \n- \nO still loading\n"));
        // A shell with no shell-init line never marks anything, which is the
        // case the ceiling exists for.
        assert!(!has_drawn_a_prompt(""));
    }

    #[test]
    fn every_command_is_preceded_by_a_wait_for_the_pane_it_goes_to() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");

        let mut sends = 0;
        for (i, step) in built.commands.iter().enumerate() {
            let Step::Tmux(cmd) = step else { continue };
            if cmd.first().map(String::as_str) != Some("send-keys") {
                continue;
            }
            sends += 1;
            let at = &cmd[2];
            assert_eq!(
                built.commands.get(i - 1),
                Some(&Step::WaitForPrompt(at.clone())),
                "a send-keys to {at} was not preceded by a wait for it"
            );
        }
        assert_eq!(sends, 3);
    }

    #[test]
    fn a_pane_nothing_is_sent_to_is_never_waited_for() {
        // Waiting costs the ceiling on a shell with no prompt marks, so a pane
        // opening at a prompt must not pay for one.
        let mut snap = two_edits();
        for window in &mut snap.session[0].window {
            for pane in &mut window.pane {
                pane.command = String::new();
            }
        }
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        assert!(
            !built
                .commands
                .iter()
                .any(|s| matches!(s, Step::WaitForPrompt(_)))
        );
    }

    #[test]
    fn the_ceiling_is_short_enough_to_be_paid_once_per_pane() {
        // Fourteen panes with no prompt marks would cost this times fourteen
        // before anything ran, so the number is part of the design rather than
        // a value somebody picked once.
        assert!(PROMPT_WAIT <= std::time::Duration::from_secs(2));
        assert!(PROMPT_POLL < PROMPT_WAIT);
    }

    #[test]
    fn a_live_server_is_refused_and_nothing_is_built() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let live = vec!["mysetup".to_string()];
        let err = rebuild(&spec(&snap, &planned, &live)).expect_err("should refuse");
        assert_eq!(err.code(), 3);
        assert!(err.to_string().contains("mysetup"), "{err}");
        assert!(err.to_string().contains("--merge"), "{err}");
    }

    #[test]
    fn merge_builds_what_is_missing_and_leaves_the_rest_alone() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let live = vec!["mysetup".to_string()];
        let mut s = spec(&snap, &planned, &live);
        s.merge = true;
        let built = rebuild(&s).expect("merge");
        assert_eq!(built.sessions, vec!["lonkar_org".to_string()]);
        assert!(!flat(&built).iter().any(|c| c.contains("mysetup")));
    }

    #[test]
    fn a_session_that_is_already_running_is_never_rebuilt_under_merge() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let live = vec!["lonkar_org".to_string()];
        let mut s = spec(&snap, &planned, &live);
        s.merge = true;
        let err = rebuild(&s).expect_err("nothing left to do");
        assert_eq!(err, Refusal::Empty);
        assert_eq!(err.code(), 4);
    }

    #[test]
    fn two_windows_with_one_name_are_targeted_by_index() {
        // The reason session_commands could not be reused. Targeting by name
        // would send both `edit` windows' work to whichever tmux found first.
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        let lines = flat(&built);
        assert!(
            lines.iter().any(|c| c.contains("=lonkar_org:3")),
            "{lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|c| c.starts_with("new-window")).count(),
            2
        );
    }

    #[test]
    fn the_order_is_windows_then_geometry_then_commands_then_selection() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        let lines = flat(&built);
        let first = |prefix: &str| lines.iter().position(|c| c.starts_with(prefix));

        let new_window = first("new-window").expect("a new-window");
        let send = first("send-keys").expect("a send-keys");
        let select_window = first("select-window").expect("a select-window");
        assert!(new_window < send, "{lines:?}");
        assert!(send < select_window, "{lines:?}");
    }

    #[test]
    fn the_conversation_is_what_gets_sent() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        assert!(
            flat(&built)
                .iter()
                .any(|c| c == "send-keys -t =lonkar_org:2.1 claude --resume abc C-m"),
            "{:?}",
            flat(&built)
        );
    }

    #[test]
    fn a_pane_nothing_claimed_is_opened_and_left_alone() {
        let mut snap = two_edits();
        snap.session[0].window[0].pane[0].command = "./deploy --production".to_string();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        assert!(
            !flat(&built).iter().any(|c| c.contains("deploy")),
            "an unclaimed command was sent"
        );
    }

    #[test]
    fn a_window_of_one_pane_gets_no_split_and_no_layout() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        let lines = flat(&built);
        assert!(!lines.iter().any(|c| c.starts_with("split-window")));
        assert!(!lines.iter().any(|c| c.starts_with("select-layout")));
    }

    #[test]
    fn a_split_window_is_rebuilt_and_its_saved_shape_applied() {
        let mut snap = two_edits();
        snap.session[0].window[0].pane = vec![pane(1, "nvim", true), pane(2, "", false)];
        snap.session[0].window[0].layout = "abcd,170x52,0,0{85x52,0,0,1,84x52,86,0,2}".to_string();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        let lines = flat(&built);
        assert_eq!(
            lines
                .iter()
                .filter(|c| c.starts_with("split-window"))
                .count(),
            1
        );
        let layout = lines
            .iter()
            .position(|c| c.starts_with("select-layout"))
            .expect("a layout");
        let split = lines
            .iter()
            .position(|c| c.starts_with("split-window"))
            .expect("a split");
        assert!(
            split < layout,
            "the shape was applied before the panes existed"
        );
    }

    #[test]
    fn a_zoomed_window_comes_back_zoomed_after_its_shape_is_set() {
        let mut snap = two_edits();
        snap.session[0].window[2].zoomed = true;
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        let lines = flat(&built);
        assert!(lines.iter().any(|c| c == "resize-pane -Z -t =lonkar_org:3"));
    }

    #[test]
    fn a_first_window_that_is_not_at_this_servers_base_index_is_moved() {
        let mut snap = two_edits();
        for (i, w) in snap.session[0].window.iter_mut().enumerate() {
            w.index = i as u32 + 5;
        }
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        assert!(
            flat(&built)
                .iter()
                .any(|c| c == "move-window -s =lonkar_org:^ -t =lonkar_org:5"),
            "{:?}",
            flat(&built)
        );
    }

    #[test]
    fn a_window_opens_where_its_own_pane_was_not_where_its_session_was() {
        // A session holding a window in a sibling checkout is ordinary, and
        // this is the shape of `lonkar_org` on the laptop these fixtures came
        // from: two windows in the project, a third in elsonsind.com. Using the
        // session path for every window restored that one to the wrong
        // repository and reported success.
        let mut snap = two_edits();
        snap.session[0].window[2].pane[0].cwd = "/w/elsewhere".to_string();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        assert!(
            flat(&built)
                .iter()
                .any(|c| c == "new-window -d -t =lonkar_org:3 -c /w/elsewhere -n edit"),
            "{:?}",
            flat(&built)
        );
    }

    #[test]
    fn a_window_with_no_pane_directory_falls_back_to_its_session() {
        let mut snap = two_edits();
        snap.session[0].window[1].pane[0].cwd = String::new();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let built = rebuild(&spec(&snap, &planned, &[])).expect("rebuild");
        assert!(
            flat(&built)
                .iter()
                .any(|c| c == "new-window -d -t =lonkar_org:2 -c /w/proj -n ai"),
            "{:?}",
            flat(&built)
        );
    }

    #[test]
    fn a_directory_that_is_not_on_this_machine_opens_at_home() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let missing = vec!["/w/proj".to_string()];
        let mut s = spec(&snap, &planned, &[]);
        s.missing = &missing;
        let built = rebuild(&s).expect("rebuild");
        assert!(
            flat(&built)
                .iter()
                .any(|c| c.contains("-c /Users/you") && c.starts_with("new-session")),
            "{:?}",
            flat(&built)
        );
    }

    #[test]
    fn missing_directories_are_found_once_each() {
        let snap = two_edits();
        let missing = missing_directories(&snap, |_| false);
        assert_eq!(missing, vec!["/w/proj".to_string()]);
        assert!(missing_directories(&snap, |_| true).is_empty());
    }

    #[test]
    fn only_picks_the_named_sessions_and_nothing_else() {
        let mut snap = two_edits();
        snap.session.push(Session {
            name: "mysetup".to_string(),
            path: "/w/other".to_string(),
            window: vec![window(1, "edit", vec![pane(1, "nvim", true)])],
        });
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let only = vec!["mysetup".to_string()];
        let mut s = spec(&snap, &planned, &[]);
        s.only = &only;
        let built = rebuild(&s).expect("rebuild");
        assert_eq!(built.sessions, vec!["mysetup".to_string()]);
    }

    #[test]
    fn exclude_drops_a_session_the_snapshot_holds() {
        let snap = two_edits();
        let table = crate::config::Restore::default();
        let planned = plan(&table, &snap);
        let exclude = vec!["lonkar_org".to_string()];
        let mut s = spec(&snap, &planned, &[]);
        s.exclude = &exclude;
        assert_eq!(rebuild(&s).expect_err("nothing left"), Refusal::Empty);
    }

    #[test]
    fn an_empty_snapshot_is_refused_with_the_code_for_nothing_to_do() {
        let snap = Snapshot::default();
        let planned: Vec<Planned> = Vec::new();
        let err = rebuild(&spec(&snap, &planned, &[])).expect_err("nothing");
        assert_eq!(err, Refusal::Empty);
        assert_eq!(err.code(), 4);
    }
}
