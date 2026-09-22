//! Sourcing tmux's config when it changes.
//!
//! The daemon already watches the tmux config, because the key rows behind
//! `keys` and `cheatsheet` are rebuilt when it is newer than they are. Noticing
//! the same change and telling tmux to source the file is the rest of a plugin
//! that otherwise exists only to run a polling loop somebody has to install.
//!
//! Credit: `b0o/tmux-autoreload` is this idea, and it watches with `entr` or
//! `inotifywait` where this compares a modification time, which is the whole
//! difference between the two.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// What the watcher knows between passes.
///
/// Kept as a value rather than in `ServerState` because nothing else reads it,
/// and a watcher whose state lives inside its own loop cannot be left
/// inconsistent by anything else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watch {
    /// The modification time of each watched file at the last pass, in the
    /// order the files were given.
    pub seen: Vec<Option<SystemTime>>,
}

/// Which of `files` changed since the last pass, updating what is remembered.
///
/// A file that did not exist and now does counts as changed, because that is
/// somebody creating the config for the first time. A file that existed and now
/// does not counts as changed too, but sourcing it would fail, so the caller
/// only sources what it can read.
///
/// The first pass changes nothing. Without that, a daemon started after a
/// config edit would source the file at startup, which is a surprise and
/// occasionally a loop when the config itself starts a daemon.
pub fn changed(
    watch: &mut Watch,
    files: &[PathBuf],
    now: &dyn Fn(&Path) -> Option<SystemTime>,
) -> Vec<PathBuf> {
    let fresh: Vec<Option<SystemTime>> = files.iter().map(|f| now(f)).collect();
    let first_pass = watch.seen.len() != files.len();
    let out = if first_pass {
        Vec::new()
    } else {
        files
            .iter()
            .zip(&fresh)
            .zip(&watch.seen)
            .filter(|((_, new), old)| new != old)
            .map(|((f, _), _)| f.clone())
            .collect()
    };
    watch.seen = fresh;
    out
}

/// The files to watch, given a config list and a fallback.
///
/// An empty list in the config means "the one the daemon already watches",
/// rather than meaning "watch nothing", because a table somebody left blank is
/// almost always a table they have not filled in yet.
pub fn files_to_watch(configured: &[PathBuf], default: PathBuf) -> Vec<PathBuf> {
    if configured.is_empty() {
        vec![default]
    } else {
        configured.to_vec()
    }
}

/// The task: notice a change, tell tmux to source the file.
pub async fn autoreload_loop(settings: crate::config::Autoreload, default_file: PathBuf) {
    let interval = Duration::from_secs(settings.interval_secs.max(1));
    let files = files_to_watch(&settings.files, default_file);
    let mut watch = Watch::default();
    let mtime = |p: &Path| crate::keys::config_mtime(p);
    loop {
        tokio::time::sleep(interval).await;
        for file in changed(&mut watch, &files, &mtime) {
            if !file.exists() {
                continue;
            }
            source_file(&file).await;
        }
    }
}

/// `tmux source-file`, and a message in tmux when it fails.
///
/// The message goes to tmux rather than to the daemon's stderr, which nobody
/// is reading. A config with a syntax error in it is exactly the moment
/// somebody needs to be told, and the reload they just triggered is the only
/// reason they would be looking.
pub async fn source_file(file: &Path) {
    let out = tokio::process::Command::new("tmux")
        .args(["source-file", &file.display().to_string()])
        .output()
        .await;
    let Ok(out) = out else { return };
    if out.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&out.stderr);
    let first = text.lines().next().unwrap_or("source-file failed");
    let _ = tokio::process::Command::new("tmux")
        .args(["display-message", &format!("tmux-companion: {first}")])
        .status()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> Option<SystemTime> {
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }

    fn fixed(times: Vec<Option<SystemTime>>) -> impl Fn(&Path) -> Option<SystemTime> {
        move |p: &Path| {
            let i: usize = p.file_name().unwrap().to_str().unwrap().parse().unwrap();
            times[i]
        }
    }

    fn files(n: usize) -> Vec<PathBuf> {
        (0..n).map(|i| PathBuf::from(i.to_string())).collect()
    }

    #[test]
    fn the_first_pass_reloads_nothing() {
        // A daemon started after somebody edited their config would otherwise
        // source it at startup, which is a surprise and, when the config is
        // what starts the daemon, a loop.
        let mut w = Watch::default();
        assert!(changed(&mut w, &files(2), &fixed(vec![t(1), t(2)])).is_empty());
        assert_eq!(w.seen, vec![t(1), t(2)]);
    }

    #[test]
    fn an_unchanged_file_reloads_nothing() {
        let mut w = Watch::default();
        let f = files(1);
        changed(&mut w, &f, &fixed(vec![t(1)]));
        assert!(changed(&mut w, &f, &fixed(vec![t(1)])).is_empty());
    }

    #[test]
    fn a_newer_modification_time_is_a_change() {
        let mut w = Watch::default();
        let f = files(1);
        changed(&mut w, &f, &fixed(vec![t(1)]));
        assert_eq!(changed(&mut w, &f, &fixed(vec![t(2)])), f);
    }

    #[test]
    fn an_older_modification_time_is_a_change_too() {
        // Restoring a config from a backup moves the time backwards, and that
        // is still somebody changing their config.
        let mut w = Watch::default();
        let f = files(1);
        changed(&mut w, &f, &fixed(vec![t(5)]));
        assert_eq!(changed(&mut w, &f, &fixed(vec![t(2)])), f);
    }

    #[test]
    fn a_file_appearing_is_a_change() {
        let mut w = Watch::default();
        let f = files(1);
        changed(&mut w, &f, &fixed(vec![None]));
        assert_eq!(changed(&mut w, &f, &fixed(vec![t(1)])), f);
    }

    #[test]
    fn a_file_disappearing_is_a_change_that_the_caller_declines_to_source() {
        let mut w = Watch::default();
        let f = files(1);
        changed(&mut w, &f, &fixed(vec![t(1)]));
        // Reported, and the loop skips it because it does not exist.
        assert_eq!(changed(&mut w, &f, &fixed(vec![None])), f);
    }

    #[test]
    fn only_the_file_that_changed_comes_back() {
        let mut w = Watch::default();
        let f = files(3);
        changed(&mut w, &f, &fixed(vec![t(1), t(1), t(1)]));
        assert_eq!(
            changed(&mut w, &f, &fixed(vec![t(1), t(9), t(1)])),
            vec![PathBuf::from("1")]
        );
    }

    #[test]
    fn a_change_is_reported_once_and_not_again() {
        let mut w = Watch::default();
        let f = files(1);
        changed(&mut w, &f, &fixed(vec![t(1)]));
        assert_eq!(changed(&mut w, &f, &fixed(vec![t(2)])), f);
        assert!(changed(&mut w, &f, &fixed(vec![t(2)])).is_empty());
    }

    #[test]
    fn an_empty_list_watches_the_file_the_daemon_already_watches() {
        // A table somebody left blank is a table they have not filled in yet,
        // not an instruction to watch nothing.
        let d = PathBuf::from("/home/me/.config/tmux/tmux.conf");
        assert_eq!(files_to_watch(&[], d.clone()), vec![d]);
    }

    #[test]
    fn a_configured_list_replaces_the_default_rather_than_adding_to_it() {
        let mine = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        assert_eq!(files_to_watch(&mine, PathBuf::from("/default")), mine);
    }

    #[test]
    fn the_watch_survives_a_list_that_grows() {
        // Changing the config list between passes must not compare an old
        // time against a new file, which would reload something at random.
        let mut w = Watch::default();
        changed(&mut w, &files(1), &fixed(vec![t(1), t(1)]));
        assert!(changed(&mut w, &files(2), &fixed(vec![t(1), t(1)])).is_empty());
        assert_eq!(w.seen.len(), 2);
    }
}
