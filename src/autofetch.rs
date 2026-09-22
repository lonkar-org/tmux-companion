//! Fetching in the background, so ahead and behind mean something.
//!
//! The git segment reports how far a branch is from its upstream, and that
//! number is only as fresh as the last fetch. A bar that says "up to date"
//! because nothing has fetched in a week is worse than a bar that says nothing,
//! because the first one is believed.
//!
//! This is a daemon task rather than a plugin because the plugin doing it today
//! is a shell script on a timer, and the daemon already has a timer, a runtime
//! and the list of repositories the bar has drawn. It is off by default: it is
//! the only part of this tool that touches the network.
//!
//! Credit where it is due: `thepante/tmux-git-autofetch` is the plugin this
//! idea comes from, it is alive, and somebody not running tmux-companion should
//! install theirs.

use std::{path::PathBuf, time::Duration};

/// The git invocation that fetches one repository.
///
/// `--quiet` because nothing reads the output, and no `--prune` because
/// deleting somebody's stale remote refs behind their back is a bigger decision
/// than keeping a count accurate.
pub fn fetch_args(repo: &std::path::Path) -> Vec<String> {
    vec![
        "-C".to_string(),
        repo.display().to_string(),
        "fetch".to_string(),
        "--quiet".to_string(),
    ]
}

/// The environment a background fetch has to run in.
///
/// Every one of these exists to stop git asking a question. There is no
/// terminal on the other end of a daemon, so a credential prompt, an askpass
/// helper or an ssh passphrase prompt does not fail: it waits, holding the pass
/// open until the timeout, every time, for as long as the daemon runs.
pub fn fetch_env() -> Vec<(&'static str, &'static str)> {
    vec![
        // git's own prompt, and the one that catches most HTTPS remotes.
        ("GIT_TERMINAL_PROMPT", "0"),
        // The helper git reaches for when it cannot prompt directly.
        ("GIT_ASKPASS", ""),
        ("SSH_ASKPASS", ""),
        // ssh's passphrase and host-key prompts, which are the ones that hang
        // a fetch over ssh rather than failing it.
        (
            "GIT_SSH_COMMAND",
            "ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new",
        ),
        // Git Credential Manager, which has its own idea about interactivity.
        ("GCM_INTERACTIVE", "never"),
    ]
}

/// Fetch one repository, giving up after `timeout`.
///
/// Answers whether git reported success. A failure is not logged at this level
/// and is not an error anywhere: a repository with no remote, a laptop on a
/// train and a remote that is briefly down all look the same from here, and
/// none of them is worth a line in somebody's terminal.
pub async fn fetch_one(repo: &std::path::Path, timeout: Duration) -> bool {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(fetch_args(repo))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (k, v) in fetch_env() {
        cmd.env(k, v);
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status.success(),
        // Timed out. The child is killed rather than left behind, because a
        // hung fetch every interval would otherwise accumulate git processes
        // for as long as the daemon runs.
        Err(_) => {
            let _ = child.kill().await;
            false
        }
        Ok(Err(_)) => false,
    }
}

/// The repository root a path is in, or `None` when it is not in one.
pub async fn repo_root(path: &std::path::Path) -> Option<PathBuf> {
    let out = tokio::process::Command::new("git")
        .args([
            "-C",
            &path.display().to_string(),
            "rev-parse",
            "--show-toplevel",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// The distinct repository roots behind a list of directories.
///
/// The bar records the pane's directory, not the root, so two panes in two
/// subdirectories of one tree arrive here as two entries and would otherwise be
/// two network fetches of the same remote every pass. Resolving costs one local
/// git call each, which against an interval measured in minutes is free.
pub async fn roots_of(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        if let Some(root) = repo_root(&p).await
            && seen.insert(root.clone())
        {
            out.push(root);
        }
    }
    out
}

/// The task: fetch every repository the bar has drawn lately, then wait.
///
/// Sequential on purpose. The point is that the counts are right by the time
/// somebody looks, not that they are right quickly, and a dozen fetches at once
/// is a spike in network and in git processes for no gain anybody sees.
pub async fn autofetch_loop(
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
    settings: crate::config::Autofetch,
) {
    let interval = Duration::from_secs(settings.interval_secs.max(1));
    let timeout = Duration::from_secs(settings.timeout_secs.max(1));
    let remember = Duration::from_secs(settings.remember_secs.max(1));
    loop {
        tokio::time::sleep(interval).await;
        // Copied out from under the lock, because the fetches below are awaits
        // and a guard must never be held across one.
        let seen: Vec<PathBuf> = state.lock().await.repos_to_fetch(remember);
        for repo in roots_of(seen).await {
            fetch_one(&repo, timeout).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_command_names_the_repository_rather_than_relying_on_a_working_directory() {
        // The daemon's own working directory is wherever it was started, which
        // is never the repository being fetched.
        let args = fetch_args(Path::new("/w/proj"));
        assert_eq!(args[0], "-C");
        assert_eq!(args[1], "/w/proj");
        assert!(args.contains(&"fetch".to_string()));
    }

    #[test]
    fn nothing_prunes_because_deleting_refs_is_not_this_feature_s_decision() {
        let args = fetch_args(Path::new("/w/proj"));
        assert!(!args.iter().any(|a| a.contains("prune")), "{args:?}");
    }

    #[test]
    fn every_way_git_can_ask_a_question_is_turned_off() {
        // A prompt on a daemon does not fail, it waits, and it waits every
        // interval for as long as the daemon runs.
        let env = fetch_env();
        let keys: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
        for k in [
            "GIT_TERMINAL_PROMPT",
            "GIT_ASKPASS",
            "SSH_ASKPASS",
            "GIT_SSH_COMMAND",
        ] {
            assert!(keys.contains(&k), "{k} is not disabled: {keys:?}");
        }
        let ssh = env
            .iter()
            .find(|(k, _)| *k == "GIT_SSH_COMMAND")
            .map(|(_, v)| *v)
            .unwrap();
        assert!(ssh.contains("BatchMode=yes"), "{ssh}");
        assert_eq!(
            env.iter()
                .find(|(k, _)| *k == "GIT_TERMINAL_PROMPT")
                .map(|(_, v)| *v),
            Some("0")
        );
    }

    #[tokio::test]
    async fn two_directories_in_one_tree_are_fetched_once() {
        // Two panes in two subdirectories is the normal case, and without this
        // it is two network round trips to the same remote every pass.
        let dir = tempfile::tempdir().unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return;
        }
        let sub = dir.path().join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        let roots = roots_of(vec![dir.path().to_path_buf(), sub]).await;
        assert_eq!(roots.len(), 1, "{roots:?}");
    }

    #[tokio::test]
    async fn a_directory_outside_any_repository_contributes_no_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(roots_of(vec![dir.path().to_path_buf()]).await.is_empty());
    }

    #[tokio::test]
    async fn a_directory_that_is_not_a_repository_fails_quietly() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!fetch_one(dir.path(), Duration::from_secs(5)).await);
    }

    #[tokio::test]
    async fn a_repository_with_no_remote_returns_rather_than_hanging() {
        // `git fetch` with nothing to fetch from exits 0 and does nothing, so
        // the thing worth asserting is that it comes back at all: a fetch that
        // hangs is the failure that holds a pass open.
        let dir = tempfile::tempdir().unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return; // no git on this machine, and that is not this test's problem
        }
        let start = std::time::Instant::now();
        fetch_one(dir.path(), Duration::from_secs(10)).await;
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "it should have returned, not timed out"
        );
    }

    #[tokio::test]
    async fn a_fetch_that_hangs_is_killed_at_the_timeout() {
        // Not a real repository, so this proves the timeout path rather than
        // git's own behaviour: a `git` that never returns must not hold the
        // pass open, and must not be left behind to accumulate.
        let dir = tempfile::tempdir().unwrap();
        let start = std::time::Instant::now();
        assert!(!fetch_one(dir.path(), Duration::from_millis(1)).await);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }
}
