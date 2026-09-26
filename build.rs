//! Stamps a build identifier into the binary.
//!
//! The version alone cannot answer "is this daemon from the build I just
//! installed": during development every build carries the same version, and
//! an upgraded binary on disk changes nothing until the old process exits. The
//! stamp makes the mismatch visible, which is what lets a client restart a
//! stale daemon instead of quietly getting an older answer.
//!
//! The stamp is `<seconds>.<commit>`, with `-dirty` on the end when the tree
//! had uncommitted changes: `1790400979.abc1234-dirty`. The seconds come first
//! and are what a client compares, so they stay a plain run of digits right
//! after the `+`. The commit is for the person reading a `doctor` report or a
//! snapshot header, since two machines building one commit get two different
//! second counts and nothing else in the id said they were the same code.
//! Without git the stamp is the seconds alone.

use std::{
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stamp = match commit() {
        Some(commit) if dirty() => format!("{secs}.{commit}-dirty"),
        Some(commit) => format!("{secs}.{commit}"),
        None => secs.to_string(),
    };
    println!("cargo:rustc-env=TMUX_COMPANION_BUILD={stamp}");

    // Without this the stamp would be baked once and never refreshed, which is
    // the opposite of what it is for.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // A commit changes the stamp without touching a source file, so the ref
    // HEAD points at is watched too. Only when it is a file: a ref packed
    // away by `git gc` has no file, and naming one that does not exist makes
    // cargo rerun this on every build.
    for path in ref_files() {
        if std::path::Path::new(&path).is_file() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

/// Run git in the package directory and take its output, or nothing when
/// git is absent, this is not a checkout, or the command failed.
fn git(args: &[&str]) -> Option<String> {
    let dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The short commit, or nothing outside a checkout.
fn commit() -> Option<String> {
    git(&["rev-parse", "--short", "HEAD"]).filter(|s| !s.is_empty())
}

/// Whether a tracked file differs from the commit.
///
/// Untracked files do not count, which is what `git describe --dirty` does
/// too: a scratch file beside the sources is not a change to the sources.
fn dirty() -> bool {
    git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty())
}

/// The files git changes when HEAD moves: HEAD itself, and the branch ref it
/// names when it names one.
///
/// Through `--git-path` rather than `.git/`, because in a worktree `.git` is
/// a file pointing elsewhere and the refs live in the common directory.
fn ref_files() -> Vec<String> {
    let mut files = Vec::new();
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        files.push(head);
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &branch])
    {
        files.push(path);
    }
    files
}
