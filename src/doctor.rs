//! `tmux-companion doctor`: the first thing to ask for on an issue from a
//! stranger.
//!
//! Everything here is a question somebody would otherwise be asked one at a
//! time over three days: which binary, which daemon, which socket, which
//! config, which tmux, which glyphs.

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
    let _ = writeln!(out, "  tmux          {}", tmux_version());
    let _ = writeln!(out, "  platform      {}", platform());
    out
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
        Ok(r) => r.output.lines().collect::<Vec<_>>().join("; "),
        Err(e) => format!("not answering: {e}"),
    }
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
    match std::process::Command::new("tmux").arg("-V").output() {
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
            "tmux",
            "platform",
        ] {
            assert!(r.contains(line), "`{line}` missing from:\n{r}");
        }
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
