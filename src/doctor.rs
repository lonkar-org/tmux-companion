//! `tmux-companion doctor`: the first thing to ask for on an issue from a
//! stranger.
//!
//! Everything here is a question somebody would otherwise be asked one at a
//! time over three days: which binary, which daemon, which socket, which
//! config, which tmux, which glyphs.
//!
//! The last section reads tmux's own options and says which ones cost
//! something as they are set. `tmux-plugins/tmux-sensible` is where most
//! people got those settings from, by having a plugin set them, and it has
//! not been pushed since April 2024. Nothing is set from here: each line
//! names the option, what it costs, and the line for tmux.conf, and whether
//! to add it is for whoever owns the file.

use std::fmt::Write as _;

/// Gather the report and print it.
pub async fn run() -> anyhow::Result<()> {
    print!("{}", report().await);
    Ok(())
}

/// The report, as a string, so a test can read it without capturing stdout.
pub async fn report() -> String {
    let mut out = String::new();

    let _ = writeln!(out, "tmux-companion {}", crate::proto::build_id());
    let _ = writeln!(out, "  binary        {}", current_exe());
    let _ = writeln!(out, "  daemon        {}", daemon_state().await);
    let _ = writeln!(out, "  socket        {}", socket_state());
    let _ = writeln!(out, "  config        {}", config_state());
    let _ = writeln!(out, "  glyphs        {}", glyph_state());
    let _ = writeln!(out, "  state dir     {}", state_dir_state());
    let _ = writeln!(out, "  daemon log    {}", log_state());
    let _ = writeln!(out, "  sessions      {}", sessions_state());
    let _ = writeln!(out, "  autosave      {}", autosave_state());
    let _ = writeln!(out, "  health        {}", health_state().await);
    let _ = writeln!(out, "  setup         {}", setup_state().await);
    let _ = writeln!(out, "  tmux          {}", tmux_version());
    let _ = writeln!(out, "  platform      {}", platform());
    out.push_str(&options_section(tmux_options().as_ref()));
    out
}

/// One thing about tmux's own options that is worth changing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    /// The option and the value it has.
    pub found: String,
    /// What that costs.
    pub why: String,
    /// The line for tmux.conf.
    pub fix: &'static str,
}

/// tmux's options by name, quotes taken off the values.
pub fn parse_options(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (name, value) = l.trim().split_once(' ')?;
            Some((
                name.to_string(),
                value
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string(),
            ))
        })
        .collect()
}

/// What is worth changing in a set of options.
///
/// An option that is not there is an older tmux that doesn't have it, and
/// gets no line. A number that doesn't parse gets none either: the point is
/// advice somebody can trust, and a guess about a value this couldn't read
/// isn't that.
pub fn advice(options: &std::collections::HashMap<String, String>) -> Vec<Advice> {
    let get = |name: &str| options.get(name).map(String::as_str);
    let number = |name: &str| get(name).and_then(|v| v.parse::<u64>().ok());
    let mut out = Vec::new();
    let mut say = |found: String, why: String, fix: &'static str| {
        out.push(Advice { found, why, fix });
    };

    if let Some(ms) = number("escape-time").filter(|ms| *ms > 50) {
        say(
            format!("escape-time {ms}"),
            format!("Esc waits {ms} ms before the program in the pane sees it"),
            "set -s escape-time 10",
        );
    }
    if let Some(lines) = number("history-limit").filter(|l| *l <= 2000) {
        say(
            format!("history-limit {lines}"),
            format!(
                "a pane keeps {lines} lines, one long build, and `search` reads only what is kept"
            ),
            "set -g history-limit 50000",
        );
    }
    if let Some(ms) = number("display-time").filter(|ms| *ms < 2000) {
        say(
            format!("display-time {ms}"),
            format!("a message stays {ms} ms, and that is where notify and the nudges speak"),
            "set -g display-time 4000",
        );
    }
    match number("status-interval") {
        Some(0) => say(
            "status-interval 0".to_string(),
            "the bar is redrawn only when something else redraws it".to_string(),
            "set -g status-interval 1",
        ),
        Some(secs) if secs > 5 => say(
            format!("status-interval {secs}"),
            format!("the bar is {secs} s old by the time it is redrawn, the net rate with it"),
            "set -g status-interval 1",
        ),
        _ => {}
    }
    if get("focus-events") == Some("off") {
        say(
            "focus-events off".to_string(),
            "an editor in a pane isn't told when you come back to it, so it can't reread a file"
                .to_string(),
            "set -s focus-events on",
        );
    }
    if let Some(term) = get("default-terminal").filter(|t| !t.contains("256color")) {
        say(
            format!("default-terminal {term}"),
            "programs in a pane are told the terminal has 8 colours".to_string(),
            "set -s default-terminal \"screen-256color\"",
        );
    }
    if get("set-clipboard") == Some("off") {
        say(
            "set-clipboard off".to_string(),
            "a copy never reaches the terminal's clipboard, which over ssh is the only one"
                .to_string(),
            "set -s set-clipboard external",
        );
    }
    if get("monitor-bell") == Some("off") {
        say(
            "monitor-bell off".to_string(),
            "an agent that rings the bell when it asks isn't seen asking".to_string(),
            "set -gw monitor-bell on",
        );
    }
    if get("mouse") == Some("off") {
        say(
            "mouse off".to_string(),
            "a click on a segment of the bar does nothing, if you wanted it to".to_string(),
            "set -g mouse on",
        );
    }
    out
}

/// The options section of the report.
///
/// `None` is a tmux that would not answer, which outside a server is the
/// ordinary case and gets one line saying so.
pub fn options_section(options: Option<&std::collections::HashMap<String, String>>) -> String {
    let mut out = String::from("tmux options\n");
    let Some(options) = options else {
        out.push_str("  no tmux server to ask\n");
        return out;
    };
    let advice = advice(options);
    if advice.is_empty() {
        out.push_str("  nothing to suggest\n");
        return out;
    }
    let width = advice.iter().map(|a| a.found.len()).max().unwrap_or(0);
    for a in &advice {
        let _ = writeln!(out, "  {:<width$}  {}", a.found, a.why);
        let _ = writeln!(out, "  {:<width$}  {}", "", a.fix);
    }
    out
}

/// The server, session and window options of the running tmux.
fn tmux_options() -> Option<std::collections::HashMap<String, String>> {
    let mut all = String::new();
    for scope in ["-s", "-g", "-gw"] {
        let out = crate::tmux::command_sync()
            .args(["show-options", scope])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        all.push_str(&String::from_utf8_lossy(&out.stdout));
        all.push('\n');
    }
    Some(parse_options(&all))
}

fn current_exe() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("unknown ({e})"))
}

/// Ask the running daemon what build it is, without starting one or replacing
/// one.
///
/// Deliberately not `client::send`: that starts a daemon when none answers and
/// replaces one from an older build. Both are wrong here, because "what is
/// running" is the question and a diagnostic that changes the answer while
/// reading it is not a diagnostic.
async fn daemon_state() -> String {
    let sock = crate::client::sock_path();
    let Ok(_) = tokio::net::UnixStream::connect(&sock).await else {
        return "not running".to_string();
    };
    match crate::client::send_once(&crate::proto::Request::raw("noop", serde_json::Value::Null))
        .await
    {
        Ok(r) if r.version.is_empty() => "running, from a build too old to say which".to_string(),
        Ok(r) if r.version == crate::proto::build_id() => {
            format!("running, {} (this one)", r.version)
        }
        Ok(r) => format!(
            "running, {} (this binary is {}); run tmux-companion restart",
            r.version,
            crate::proto::build_id()
        ),
        Err(e) => format!("running but not answering: {e}; run tmux-companion restart"),
    }
}

fn socket_state() -> String {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let sock = crate::client::sock_path();
    let path = sock.display().to_string();
    match std::fs::metadata(&sock) {
        Ok(m) => {
            let mode = m.permissions().mode() & 0o777;
            let us = nix::unistd::getuid().as_raw();
            let owner = if m.uid() == us {
                "yours".to_string()
            } else {
                format!("uid {}, NOT yours", m.uid())
            };
            format!("{path} (mode {mode:04o}, {owner})")
        }
        Err(_) => format!("{path} (absent)"),
    }
}

fn config_state() -> String {
    match crate::config::load() {
        Ok((_, source)) => {
            let note = match &source {
                crate::config::Source::File(path) if edited_after_daemon_started(path) => {
                    " (changed after the daemon started; run tmux-companion restart)"
                }
                _ => "",
            };
            format!("{source}{note}")
        }
        Err(e) => format!("{} — BROKEN: {}", e.path.display(), e.message),
    }
}

/// Whether the config file was written after the running daemon read it.
///
/// The daemon reads config.toml once at startup and holds it, so a file that
/// parses and has been edited since is the one state where `config check`
/// says ok and the bar still draws yesterday's settings. The daemon's start
/// is the mtime of the marker the sessions timer writes when it comes up;
/// no marker means no daemon to be behind.
fn edited_after_daemon_started(config: &std::path::Path) -> bool {
    let Some(dir) = crate::server::state_dir() else {
        return false;
    };
    let marker = crate::sessions::timer::marker_path_in(&dir);
    newer_than(mtime(config), mtime(&marker))
}

/// A file's modification time, or nothing when it cannot be read.
fn mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// `file` was written after `daemon_start`, when both are known.
///
/// Missing on either side is "no", because the answer is a nudge to restart
/// and a nudge on a machine with no daemon running is noise.
fn newer_than(
    file: Option<std::time::SystemTime>,
    daemon_start: Option<std::time::SystemTime>,
) -> bool {
    match (file, daemon_start) {
        (Some(f), Some(d)) => f > d,
        _ => false,
    }
}

fn glyph_state() -> String {
    let Ok((config, _)) = crate::config::load() else {
        return "unknown, the config does not parse".to_string();
    };
    let map = crate::config::GlyphMap::new(&config.glyphs);
    let preset = match config.glyphs.preset {
        crate::config::Preset::NerdFontV3 => "nerd-font-v3 (needs a patched font)",
        crate::config::Preset::Ascii => "ascii",
    };
    if map.is_identity() {
        format!("{preset}, no overrides")
    } else {
        format!("{preset}, {} substitutions", config.glyphs.icons.len())
    }
}

/// The daemon log and its last line, which is the failure somebody is here
/// about more often than not.
fn log_state() -> String {
    let Some(path) = crate::server::daemon_log_path() else {
        return "nowhere, no state directory".to_string();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match text.lines().rev().find(|l| !l.trim().is_empty()) {
            Some(last) => format!("{} (last: {})", path.display(), last.trim()),
            None => format!("{} (empty)", path.display()),
        },
        Err(_) => format!("{} (not written yet)", path.display()),
    }
}

/// Whether `[sessions]` snapshots on a timer, and when the last one was.
fn sessions_state() -> String {
    let Ok((config, _)) = crate::config::load() else {
        return "unknown, the config does not parse".to_string();
    };
    use crate::config::SessionsAutosave as A;
    let timer = match config.sessions.autosave {
        A::Off => "autosave off".to_string(),
        A::Interval => format!("autosave every {}s", config.sessions.interval_secs),
        A::Cron => format!("autosave on cron {}", config.sessions.cron),
    };
    let last = crate::server::state_dir()
        .and_then(|d| crate::sessions::store::last_stamp_in(&d))
        .map(|s| format!("last snapshot {s}"))
        .unwrap_or_else(|| "no snapshot yet".to_string());
    format!("{timer}, {last}")
}

/// The deprecated `[autosave]` timer, named here because it is the one whose
/// failures went unseen for a day.
fn autosave_state() -> String {
    let Ok((config, _)) = crate::config::load() else {
        return "unknown, the config does not parse".to_string();
    };
    if !config.autosave.enabled {
        return "[autosave] off".to_string();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let script = config.autosave.script_path(&home);
    let script_state = if script.exists() {
        "present"
    } else {
        "MISSING"
    };
    format!(
        "[autosave] on (deprecated) every {}s, {}; script {} ({script_state})",
        config.autosave.interval_secs,
        crate::tasks::last_save(),
        script.display()
    )
}

/// What the health mark would say, asked of the running daemon.
///
/// The daemon holds the two things a client cannot see: when it started and
/// which timer last failed. A daemon that is not there has no health to
/// report, and a daemon too old to answer `__health` says so.
async fn health_state() -> String {
    let sock = crate::client::sock_path();
    let Ok(_) = tokio::net::UnixStream::connect(&sock).await else {
        return "no daemon to ask".to_string();
    };
    match crate::client::send_once(&crate::proto::Request::raw(
        "__health",
        serde_json::Value::Null,
    ))
    .await
    {
        Ok(r) if r.error.is_some() => "the daemon predates the health check".to_string(),
        Ok(r) if r.output.trim().is_empty() => "ok".to_string(),
        Ok(r) => health_line(&r.output),
        Err(e) => format!("not answering: {e}"),
    }
}

/// The reasons on one line, ending on the command that forgets a failure
/// when one of them is a failure.
///
/// The other reasons already end on what to run, and a failure had nothing:
/// the mark stayed for its hour whether or not anybody had read it.
fn health_line(reasons: &str) -> String {
    let mut line = reasons.lines().collect::<Vec<_>>().join("; ");
    if reasons.lines().any(|r| r.starts_with("a timer failed")) {
        line.push_str("; tmux-companion health ack forgets a failure");
    }
    line
}

/// How many of the things `setup` lists are open, by the same rules it uses.
async fn setup_state() -> String {
    let (open, total) = crate::setup::count().await;
    crate::setup::doctor_line(open, total)
}

fn state_dir_state() -> String {
    let Some(dir) = crate::server::state_dir() else {
        return "unknown, neither XDG_STATE_HOME nor HOME is set".to_string();
    };
    let last_error = dir.join("last-error");
    match std::fs::read_to_string(&last_error) {
        Ok(text) if !text.trim().is_empty() => format!(
            "{} (last config error: {})",
            dir.display(),
            text.lines().next().unwrap_or("").trim()
        ),
        _ => dir.display().to_string(),
    }
}

fn tmux_version() -> String {
    match crate::tmux::command_sync().arg("-V").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(_) => "installed, but `tmux -V` failed".to_string(),
        Err(_) => "not found on PATH".to_string(),
    }
}

fn platform() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_health_line_names_ack_only_when_something_failed() {
        let failed = "a timer failed: sessions autosave: disk full\nconfig.toml changed";
        let line = health_line(failed);
        assert!(line.starts_with("a timer failed: sessions"), "{line}");
        assert!(line.ends_with("health ack forgets a failure"), "{line}");
        let state = health_line("quiet for 40m");
        assert_eq!(state, "quiet for 40m");
    }

    #[tokio::test]
    async fn the_report_answers_every_question_it_promises() {
        let r = report().await;
        for line in [
            "binary",
            "daemon",
            "socket",
            "config",
            "glyphs",
            "state dir",
            "setup",
            "tmux",
            "platform",
        ] {
            assert!(r.contains(line), "`{line}` missing from:\n{r}");
        }
    }

    /// What a tmux 3.7 started with no config answers.
    const DEFAULTS: &str = "default-terminal tmux-256color\nescape-time 10\n\
                            focus-events off\nset-clipboard external\n\
                            display-time 750\nhistory-limit 2000\nmouse off\n\
                            status-interval 15\nmonitor-bell on\n";

    /// What tmux-sensible would have left behind, and the examples here.
    const TUNED: &str = "default-terminal \"screen-256color\"\nescape-time 0\n\
                         focus-events on\nset-clipboard external\n\
                         display-time 4000\nhistory-limit 50000\nmouse on\n\
                         status-interval 1\nmonitor-bell on\n";

    #[test]
    fn a_bare_tmux_gets_a_line_for_each_default_that_costs_something() {
        let got = advice(&parse_options(DEFAULTS));
        let found: Vec<&str> = got.iter().map(|a| a.found.as_str()).collect();
        assert_eq!(
            found,
            vec![
                "history-limit 2000",
                "display-time 750",
                "status-interval 15",
                "focus-events off",
                "mouse off",
            ]
        );
        let history = &got[0];
        assert!(history.why.contains("2000 lines"), "{history:?}");
        assert_eq!(history.fix, "set -g history-limit 50000");
    }

    #[test]
    fn a_tuned_tmux_gets_nothing_and_the_section_says_so() {
        let options = parse_options(TUNED);
        assert!(advice(&options).is_empty());
        assert_eq!(
            options_section(Some(&options)),
            "tmux options\n  nothing to suggest\n"
        );
        assert_eq!(
            options_section(None),
            "tmux options\n  no tmux server to ask\n"
        );
    }

    #[test]
    fn the_thresholds_sit_where_the_advice_says() {
        let one = |line: &str| advice(&parse_options(line));
        assert!(one("escape-time 50").is_empty());
        assert_eq!(one("escape-time 500")[0].fix, "set -s escape-time 10");
        assert!(one("history-limit 2001").is_empty());
        assert!(one("display-time 2000").is_empty());
        assert!(one("status-interval 5").is_empty());
        assert!(one("status-interval 0")[0].why.contains("only when"));
        assert_eq!(one("default-terminal screen").len(), 1);
        assert!(one("default-terminal xterm-256color").is_empty());
        assert_eq!(one("set-clipboard off").len(), 1);
        assert_eq!(one("monitor-bell off").len(), 1);
    }

    #[test]
    fn an_option_this_tmux_lacks_or_a_value_nobody_can_read_gets_no_line() {
        assert!(advice(&parse_options("")).is_empty());
        assert!(advice(&parse_options("escape-time soon\nhistory-limit\n")).is_empty());
    }

    #[test]
    fn the_section_lines_the_fix_up_under_the_reason() {
        let got = options_section(Some(&parse_options("escape-time 500\nmouse off\n")));
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(lines[0], "tmux options");
        assert_eq!(
            lines[1],
            "  escape-time 500  Esc waits 500 ms before the program in the pane sees it"
        );
        assert_eq!(lines[2], "                   set -s escape-time 10");
        assert!(lines[3].starts_with("  mouse off        "), "{got}");
    }

    #[test]
    fn a_config_edited_after_the_daemon_started_is_newer_and_nothing_else_is() {
        use std::time::{Duration, SystemTime};
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let later = start + Duration::from_secs(60);
        assert!(newer_than(Some(later), Some(start)));
        assert!(!newer_than(Some(start), Some(later)));
        assert!(!newer_than(Some(start), Some(start)));
        // No daemon, or no file: nothing to nudge about.
        assert!(!newer_than(Some(later), None));
        assert!(!newer_than(None, Some(start)));
    }

    #[test]
    fn the_platform_line_is_this_platform() {
        let p = platform();
        assert!(p.contains(std::env::consts::OS), "{p}");
        assert!(p.contains(std::env::consts::ARCH), "{p}");
    }

    #[test]
    fn the_socket_line_says_absent_rather_than_erroring() {
        // SAFETY: single-threaded test, and the variable is restored below.
        let previous = std::env::var_os("TMUX_COMPANION_SOCK");
        unsafe { std::env::set_var("TMUX_COMPANION_SOCK", "/tmp/tc-doctor-absent.sock") };
        let _ = std::fs::remove_file("/tmp/tc-doctor-absent.sock");
        assert!(socket_state().contains("absent"), "{}", socket_state());
        match previous {
            Some(v) => unsafe { std::env::set_var("TMUX_COMPANION_SOCK", v) },
            None => unsafe { std::env::remove_var("TMUX_COMPANION_SOCK") },
        }
    }
}
