//! Stopping what hangs in a pane: TERM, a wait, and KILL if it is still there.
//!
//! A program that stopped answering `C-c` leaves two moves, closing the pane
//! and losing its scrollback with it, or opening another pane to find a pid.
//! `tmux-plugins/tmux-cowboy` was the third: one key, `kill -9` on whatever
//! ran in the pane. It was last pushed in May 2021.
//!
//! This asks first. The foreground process group of the pane's terminal gets
//! TERM, which a program can catch to put the terminal back and remove its
//! lock file, and KILL only when it is still running after the grace. The
//! group is the terminal's own idea of what is in front, the same one `C-c`
//! is delivered to, so a build and the compilers it started go together and
//! a job put in the background with `&` is left alone.
//!
//! A pane with nothing in front but its own shell is refused. Killing that
//! closes the pane, which tmux already has a key for.

use std::time::Duration;

use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;

/// How long a program gets to act on TERM when nobody says.
pub const DEFAULT_GRACE_SECS: u64 = 3;

/// How often the wait looks again.
const POLL: Duration = Duration::from_millis(100);

/// One process on a pane's terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnTty {
    /// Its pid.
    pub pid: i32,
    /// Its process group.
    pub group: i32,
    /// The group the terminal has in front.
    pub front: i32,
    /// Its name, as [`name`] words it.
    pub name: String,
}

/// The columns [`parse_ps`] reads, for `ps -o`.
pub const PS_COLUMNS: &str = "pid=,pgid=,tpgid=,comm=";

/// A command as `ps` prints it, down to the name somebody would call it by.
///
/// macOS prints the whole path and marks a login shell with a dash in front,
/// so the shell in a fresh pane is `-/bin/zsh`.
pub fn name(comm: &str) -> String {
    let comm = comm.trim().trim_start_matches('-');
    comm.rsplit('/').next().unwrap_or(comm).to_string()
}

/// What `ps -o pid=,pgid=,tpgid=,comm= -t TTY` prints.
///
/// The command is last because on macOS it is a path, and a path can hold a
/// space.
pub fn parse_ps(text: &str) -> Vec<OnTty> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let pid = f.next()?.parse().ok()?;
            let group = f.next()?.parse().ok()?;
            let front = f.next()?.parse().ok()?;
            let comm = f.collect::<Vec<_>>().join(" ");
            (!comm.is_empty()).then(|| OnTty {
                pid,
                group,
                front,
                name: name(&comm),
            })
        })
        .collect()
}

/// Whether a name is a shell's.
pub fn is_shell(name: &str) -> bool {
    matches!(
        name,
        "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" | "tcsh" | "csh" | "nu"
    )
}

/// What there is to stop in a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing is in front, or the terminal could not be read.
    Nothing,
    /// Only the pane's own shell is in front.
    OnlyTheShell(String),
    /// A process group, the processes in it, and the name to call it by.
    Group {
        /// The group to signal.
        group: i32,
        /// Every process in it, to watch for.
        pids: Vec<i32>,
        /// The name of the group's leader, or of its first process when the
        /// leader has gone.
        name: String,
    },
}

/// What is in front on a pane's terminal, given the pane's own pid.
pub fn verdict(on_tty: &[OnTty], pane_pid: i32) -> Verdict {
    let front: Vec<&OnTty> = on_tty
        .iter()
        .filter(|p| p.front > 0 && p.group == p.front)
        .collect();
    let Some(first) = front.first() else {
        return Verdict::Nothing;
    };
    if front.iter().all(|p| p.pid == pane_pid && is_shell(&p.name)) {
        return Verdict::OnlyTheShell(first.name.clone());
    }
    let leader = front.iter().find(|p| p.pid == p.group).unwrap_or(first);
    Verdict::Group {
        group: first.front,
        pids: front.iter().map(|p| p.pid).collect(),
        name: leader.name.clone(),
    }
}

/// How a stop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// It went on TERM.
    Term,
    /// It was still there after the grace, and went on KILL.
    Kill,
    /// It is still there after KILL, which is a zombie or somebody else's.
    Survived,
    /// The signal could not be sent.
    Refused(nix::errno::Errno),
}

/// The sentence for an ending.
pub fn sentence(name: &str, ended: Ended, grace: Duration) -> String {
    match ended {
        Ended::Term => format!("{name} stopped on TERM"),
        Ended::Kill => format!(
            "{name} ignored TERM for {}s and was killed",
            grace.as_secs()
        ),
        Ended::Survived => format!("{name} is still there after KILL"),
        Ended::Refused(e) => format!("{name} could not be signalled: {}", e.desc()),
    }
}

/// What to signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A process group, the way the terminal delivers `C-c`.
    Group(i32),
    /// One process.
    Process(i32),
}

impl Target {
    fn signal(self, signal: Signal) -> nix::Result<()> {
        match self {
            Target::Group(g) => killpg(Pid::from_raw(g), signal),
            Target::Process(p) => kill(Pid::from_raw(p), signal),
        }
    }
}

/// Whether any of these is still a process.
///
/// Signal 0 is delivered to nobody and answers whether it could have been.
/// `EPERM` is a process that is there and belongs to somebody else.
fn any_alive(pids: &[i32]) -> bool {
    pids.iter().any(|p| {
        matches!(
            kill(Pid::from_raw(*p), None),
            Ok(()) | Err(nix::errno::Errno::EPERM)
        )
    })
}

/// Wait until none of them is left, or the time is up.
async fn gone_within(pids: &[i32], limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if !any_alive(pids) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// TERM, the grace, and KILL for what is left.
///
/// A target that has already gone when TERM is sent counts as stopped on
/// TERM: somebody asked for it to be gone and it is.
pub async fn stop(target: Target, pids: &[i32], grace: Duration) -> Ended {
    match target.signal(Signal::SIGTERM) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
        Err(e) => return Ended::Refused(e),
    }
    if gone_within(pids, grace).await {
        return Ended::Term;
    }
    match target.signal(Signal::SIGKILL) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
        Err(e) => return Ended::Refused(e),
    }
    // KILL cannot be caught, so this wait is for the kernel and the parent's
    // `wait`, and a second is generous.
    if gone_within(pids, Duration::from_secs(1)).await {
        Ended::Kill
    } else {
        Ended::Survived
    }
}

/// Say it on standard output, and on tmux's message line as well.
///
/// The message line is for the caller that is a popup: what a picker prints
/// goes when the popup closes, which is the moment the answer arrives.
async fn say_in_tmux(pane: Option<&str>, text: &str) {
    println!("{text}");
    if std::env::var_os("TMUX").is_none() {
        return;
    }
    let text = format!("tmux-companion: {text}");
    let mut args = vec!["display-message"];
    if let Some(p) = pane {
        args.extend(["-t", p]);
    }
    args.push(&text);
    crate::cli::tmux(&args).await;
}

/// Stop one process by pid, and say how it went.
pub async fn stop_process(pid: u32, name: &str, grace: Duration, pane: Option<&str>) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    let ended = stop(Target::Process(pid), &[pid], grace).await;
    say_in_tmux(pane, &sentence(name, ended, grace)).await;
}

/// The sentence for a pane with nothing to stop, or `None` when there is.
fn refusal(verdict: &Verdict, id: &str) -> Option<String> {
    match verdict {
        Verdict::Nothing => Some(format!("nothing is running in {id}")),
        Verdict::OnlyTheShell(shell) => {
            // @Yogesh(word): message when --ask finds only the shell at its prompt; has {shell} and {id}
            Some(format!(
                "only {shell} is running in {id}, and kill-pane is the key for that"
            ))
        }
        Verdict::Group { .. } => None,
    }
}

/// What `kill --ask` does with a pane, decided before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Nothing to stop: say this and ask nothing.
    Refuse(String),
    /// Something runs: ask about it by name and pid.
    Confirm {
        /// The name to call it by.
        name: String,
        /// The pid that name belongs to.
        pid: i32,
    },
}

/// What `kill --ask` does for this verdict.
///
/// The pid is the group leader's, the process the name comes from, or the
/// first one's when the leader has gone.
pub fn ask(verdict: &Verdict, id: &str) -> Ask {
    if let Some(sentence) = refusal(verdict, id) {
        return Ask::Refuse(sentence);
    }
    let Verdict::Group { group, pids, name } = verdict else {
        unreachable!("refusal answers for everything but a group");
    };
    let pid = if pids.contains(group) {
        *group
    } else {
        pids.first().copied().unwrap_or(*group)
    };
    Ask::Confirm {
        name: name.clone(),
        pid,
    }
}

/// The prompt tmux shows for a confirm, as a format: a `#` in the name is
/// doubled so tmux prints it rather than reading a format from it.
pub fn confirm_prompt(name: &str, pid: i32) -> String {
    let name = escape_format(name);
    // @Yogesh(word): confirm prompt text for --ask; has {name} and {pid}
    format!("{name} {pid} (y/n)")
}

/// A literal for a tmux format: `#` is the one character it expands.
fn escape_format(s: &str) -> String {
    s.replace('#', "##")
}

/// A word the shell hands back whole.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// A string for tmux's own command parser, in double quotes: the three
/// characters it reads inside them are escaped.
fn tmux_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The command a `y` to the confirm runs: this binary, by its own path, doing
/// the plain `kill` on the same pane.
///
/// Two format expansions happen on the way: `confirm-before` expands its
/// command when it is given, and `run-shell` expands its shell command when
/// it runs. A `#` in the path or the id is escaped for both. `--grace` goes
/// along when it is not the default.
pub fn confirm_command(me: &str, id: &str, grace_secs: u64) -> String {
    let mut shell = format!("{} kill --pane {}", shell_quote(me), shell_quote(id));
    if grace_secs != DEFAULT_GRACE_SECS {
        shell.push_str(&format!(" --grace {grace_secs}"));
    }
    escape_format(&format!("run-shell {}", tmux_quote(&escape_format(&shell))))
}

/// `kill`: stop what is in front in a pane.
///
/// With `ask`, nothing is stopped here: a pane with something in front gets
/// tmux's own confirm, naming it, whose `y` runs the plain `kill`.
pub async fn run(pane: Option<String>, grace_secs: u64, ask_first: bool) -> anyhow::Result<()> {
    let grace = Duration::from_secs(grace_secs);
    let pane = pane
        .filter(|p| !p.trim().is_empty())
        .or_else(crate::cli::pane_target);
    let answered =
        crate::cli::tmux_display_at(pane.as_deref(), "#{pane_tty}\t#{pane_pid}\t#{pane_id}").await;
    let mut fields = answered.trim().split('\t');
    let (Some(tty), Some(pane_pid), Some(id)) = (
        fields.next().filter(|t| !t.is_empty()),
        fields.next().and_then(|p| p.parse::<i32>().ok()),
        fields.next(),
    ) else {
        anyhow::bail!("not inside tmux, or the pane is gone");
    };

    let out = tokio::process::Command::new("ps")
        .args(["-o", PS_COLUMNS, "-t", tty.trim_start_matches("/dev/")])
        .output()
        .await?;
    let on_tty = parse_ps(&String::from_utf8_lossy(&out.stdout));

    // Printed and nothing more: from a binding `run-shell` shows what a
    // command printed, which is where `quiet` says its piece too.
    let verdict = verdict(&on_tty, pane_pid);
    if ask_first {
        match ask(&verdict, id) {
            Ask::Refuse(sentence) => println!("{sentence}"),
            Ask::Confirm { name, pid } => {
                // By its own path: `run-shell` has the tmux server's PATH.
                let me = std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "tmux-companion".to_string());
                // No `-t`: it takes a client, not a pane, and tmux finds the
                // one the key was pressed in the way it does for the binding.
                crate::cli::tmux(&[
                    "confirm-before",
                    "-p",
                    &confirm_prompt(&name, pid),
                    &confirm_command(&me, id, grace_secs),
                ])
                .await;
            }
        }
        return Ok(());
    }
    match verdict {
        Verdict::Nothing | Verdict::OnlyTheShell(_) => {
            if let Some(sentence) = refusal(&verdict, id) {
                println!("{sentence}");
            }
        }
        Verdict::Group { group, pids, name } => {
            let ended = stop(Target::Group(group), &pids, grace).await;
            println!("{}", sentence(&name, ended, grace));
            if matches!(ended, Ended::Survived | Ended::Refused(_)) {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_named_the_way_somebody_would_say_it() {
        assert_eq!(name("-/bin/zsh"), "zsh");
        assert_eq!(name("-zsh"), "zsh");
        assert_eq!(name("/usr/local/bin/python3"), "python3");
        assert_eq!(name("cargo"), "cargo");
        assert_eq!(name("/Applications/Some App/bin/tool"), "tool");
    }

    #[test]
    fn ps_lines_parse_and_a_path_with_a_space_stays_whole() {
        let text = "63321 63321 77891 -/bin/zsh\n\
                    77891 77891 77891 /usr/local/bin/cargo\n\
                    77900 77891 77891 /Applications/Some App/bin/rustc\n\
                    nonsense\n\
                    12 12\n";
        let got = parse_ps(text);
        assert_eq!(got.len(), 3);
        assert_eq!(
            got[0],
            OnTty {
                pid: 63321,
                group: 63321,
                front: 77891,
                name: "zsh".into()
            }
        );
        assert_eq!(got[2].name, "rustc");
        assert_eq!(got[2].group, 77891);
    }

    fn on(pid: i32, group: i32, front: i32, name: &str) -> OnTty {
        OnTty {
            pid,
            group,
            front,
            name: name.into(),
        }
    }

    #[test]
    fn a_shell_at_its_prompt_is_refused() {
        let tty = [on(100, 100, 100, "zsh")];
        assert_eq!(verdict(&tty, 100), Verdict::OnlyTheShell("zsh".into()));
    }

    #[test]
    fn the_group_in_front_goes_together_and_the_background_is_left() {
        // zsh, a build in front with a compiler under it, and a server put
        // in the background with `&`.
        let tty = [
            on(100, 100, 300, "zsh"),
            on(200, 200, 300, "node"),
            on(300, 300, 300, "cargo"),
            on(301, 300, 300, "rustc"),
        ];
        assert_eq!(
            verdict(&tty, 100),
            Verdict::Group {
                group: 300,
                pids: vec![300, 301],
                name: "cargo".into()
            }
        );
    }

    #[test]
    fn a_pane_started_on_a_program_has_that_program_to_stop() {
        // `new-window vim`: the pane's own process is the one that hung.
        let tty = [on(100, 100, 100, "vim")];
        assert_eq!(
            verdict(&tty, 100),
            Verdict::Group {
                group: 100,
                pids: vec![100],
                name: "vim".into()
            }
        );
        // And a shell that is not the pane's own is a program like another.
        let tty = [on(100, 100, 200, "zsh"), on(200, 200, 200, "bash")];
        assert!(matches!(
            verdict(&tty, 100),
            Verdict::Group { group: 200, .. }
        ));
    }

    #[test]
    fn a_terminal_with_nothing_in_front_is_nothing() {
        assert_eq!(verdict(&[], 100), Verdict::Nothing);
        // `tpgid` is -1 or 0 for a process with no terminal in front.
        assert_eq!(verdict(&[on(100, 100, 0, "zsh")], 100), Verdict::Nothing);
        assert_eq!(verdict(&[on(100, 100, -1, "zsh")], 100), Verdict::Nothing);
    }

    #[test]
    fn a_group_whose_leader_has_gone_is_named_by_what_is_left() {
        let tty = [on(100, 100, 300, "zsh"), on(301, 300, 300, "rustc")];
        assert!(matches!(
            verdict(&tty, 100),
            Verdict::Group { name, .. } if name == "rustc"
        ));
    }

    #[test]
    fn each_ending_has_its_sentence() {
        let g = Duration::from_secs(3);
        assert_eq!(sentence("cargo", Ended::Term, g), "cargo stopped on TERM");
        assert_eq!(
            sentence("cargo", Ended::Kill, g),
            "cargo ignored TERM for 3s and was killed"
        );
        assert!(sentence("cargo", Ended::Survived, g).contains("still there"));
        assert!(
            sentence("cargo", Ended::Refused(nix::errno::Errno::EPERM), g)
                .contains("could not be signalled")
        );
    }

    #[test]
    fn ask_leaves_a_shell_at_its_prompt_unasked() {
        let v = verdict(&[on(100, 100, 100, "zsh")], 100);
        assert!(matches!(ask(&v, "%45"), Ask::Refuse(s) if s.contains("zsh") && s.contains("%45")));
        assert!(matches!(ask(&Verdict::Nothing, "%45"), Ask::Refuse(_)));
    }

    #[test]
    fn ask_names_a_running_program_and_its_pid() {
        let tty = [
            on(100, 100, 300, "zsh"),
            on(300, 300, 300, "cargo"),
            on(301, 300, 300, "rustc"),
        ];
        assert_eq!(
            ask(&verdict(&tty, 100), "%45"),
            Ask::Confirm {
                name: "cargo".into(),
                pid: 300
            }
        );
        // The leader gone, the name and the pid are both the one left.
        let tty = [on(100, 100, 300, "zsh"), on(301, 300, 300, "rustc")];
        assert_eq!(
            ask(&verdict(&tty, 100), "%45"),
            Ask::Confirm {
                name: "rustc".into(),
                pid: 301
            }
        );
    }

    #[test]
    fn the_prompt_carries_the_name_and_pid_with_a_hash_kept_literal() {
        let p = confirm_prompt("cargo", 300);
        assert!(p.contains("cargo") && p.contains("300"));
        assert!(confirm_prompt("a#b", 1).contains("a##b"));
    }

    #[test]
    fn the_confirm_command_quotes_the_pane_and_carries_a_grace_that_was_given() {
        let me = "/usr/local/bin/tmux-companion";
        assert_eq!(
            confirm_command(me, "%45", DEFAULT_GRACE_SECS),
            r#"run-shell "'/usr/local/bin/tmux-companion' kill --pane '%45'""#
        );
        assert_eq!(
            confirm_command(me, "%45", 10),
            r#"run-shell "'/usr/local/bin/tmux-companion' kill --pane '%45' --grace 10""#
        );
    }

    #[test]
    fn the_confirm_command_survives_a_path_with_quotes_dollars_and_hashes() {
        // Two expansions on the way, so a `#` is four; `$`, `"` and the
        // backslash of the shell's `'\''` are escaped for tmux's parser.
        assert_eq!(
            confirm_command("/o'k/$x/#1/\"q", "%1", DEFAULT_GRACE_SECS),
            r#"run-shell "'/o'\\''k/\$x/####1/\"q' kill --pane '%1'""#
        );
    }

    /// A child in a group of its own, reaped on a thread so that a process
    /// that has exited stops being one: left to the test it would sit as a
    /// zombie and answer signal 0 for as long as the test ran.
    fn child(script: &str) -> i32 {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sh")
            .args(["-c", script])
            .process_group(0)
            .spawn()
            .expect("sh runs");
        let pid = child.id() as i32;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        pid
    }

    #[tokio::test]
    async fn a_program_that_listens_goes_on_term() {
        let pid = child("exec sleep 60");
        tokio::time::sleep(Duration::from_millis(200)).await;
        let ended = stop(Target::Group(pid), &[pid], Duration::from_secs(5)).await;
        assert_eq!(ended, Ended::Term);
        assert!(!any_alive(&[pid]));
    }

    #[tokio::test]
    async fn a_program_that_ignores_term_is_killed_after_the_grace() {
        let pid = child("trap '' TERM; while :; do sleep 1; done");
        // Long enough for the trap to be set before TERM arrives.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let ended = stop(Target::Process(pid), &[pid], Duration::from_secs(1)).await;
        assert_eq!(ended, Ended::Kill);
        assert!(!any_alive(&[pid]));
        // The `sleep` it left behind is in its group, and goes with it.
        let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
    }

    #[tokio::test]
    async fn a_process_that_has_already_gone_counts_as_stopped() {
        let pid = child("exit 0");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let ended = stop(Target::Process(pid), &[pid], Duration::from_secs(1)).await;
        assert_eq!(ended, Ended::Term);
    }
}
