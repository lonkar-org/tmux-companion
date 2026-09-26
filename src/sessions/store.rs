//! Where snapshots live, how many are kept, and which one is newest.
//!
//! Generations rather than one file, because the newest snapshot is the one a
//! crash is most likely to have caught mid-write, and a store with one copy in
//! it answers a corrupt write with nothing at all.
//!
//! The pointer to the newest is a file holding a stamp rather than a symlink.
//! A symlink is one syscall cheaper and does not survive being copied between
//! machines by anything that follows links, which these dotfiles are.

use std::path::{Path, PathBuf};

use super::{Snapshot, parse, render};

/// The directory, under a state directory, that holds every generation.
pub fn dir_in(state_dir: &Path) -> PathBuf {
    state_dir.join("sessions")
}

/// The directory holding every generation, or `None` with no state directory.
pub fn dir() -> Option<PathBuf> {
    crate::server::state_dir().map(|d| dir_in(&d))
}

/// The file one generation is written to.
pub fn path_in(state_dir: &Path, stamp: &str) -> PathBuf {
    dir_in(state_dir).join(format!("{stamp}.toml"))
}

/// The file holding the stamp of the newest generation.
pub fn pointer_in(state_dir: &Path) -> PathBuf {
    dir_in(state_dir).join("last")
}

/// A compact stamp for a file name, from a unix timestamp.
///
/// `20260925T092316`, which sorts as a string in the order it happened and
/// carries no colon, because a colon in a file name is a path separator on
/// more platforms than people remember.
pub fn stamp_from(secs: i64) -> String {
    let formatted = crate::tasks::format_unix(secs);
    let mut date = String::new();
    let mut time = String::new();
    let mut seen_space = false;
    for ch in formatted.chars() {
        match ch {
            ' ' => seen_space = true,
            c if c.is_ascii_digit() && !seen_space => date.push(c),
            c if c.is_ascii_digit() => time.push(c),
            _ => {}
        }
    }
    format!("{date}T{time}")
}

/// One generation on disk, as [`prune`] needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    /// The stamp, which is the file name without its extension.
    pub stamp: String,
    /// When it was written, in seconds since the epoch.
    pub written_at: i64,
}

/// Which generations to delete, newest-first input, oldest-first output.
///
/// `keep` counts generations and `keep_days` measures age, and a file survives
/// either one. That is deliberately the generous reading: somebody who sets a
/// short interval to survive a crash ends up with generations that span
/// minutes, and `keep_days` is how they say "an hour of those, however many
/// files that is" without doing the arithmetic themselves.
///
/// The generation the pointer names is never deleted, whatever the numbers say,
/// because deleting it would leave a store whose newest file is not the one it
/// points at.
pub fn prune(
    generations: &[Generation],
    keep: usize,
    keep_days: u32,
    now: i64,
    last: Option<&str>,
) -> Vec<String> {
    let mut sorted: Vec<&Generation> = generations.iter().collect();
    sorted.sort_by(|a, b| b.written_at.cmp(&a.written_at).then(b.stamp.cmp(&a.stamp)));

    let youngest_kept_age = i64::from(keep_days) * 86_400;
    let mut doomed = Vec::new();
    for (i, candidate) in sorted.iter().enumerate() {
        if i < keep {
            continue;
        }
        if keep_days > 0 && now.saturating_sub(candidate.written_at) <= youngest_kept_age {
            continue;
        }
        if Some(candidate.stamp.as_str()) == last {
            continue;
        }
        doomed.push(candidate.stamp.clone());
    }
    doomed.reverse();
    doomed
}

/// Every generation in a state directory, newest first.
///
/// A file that is not a snapshot, and a name that is not a stamp, are both
/// skipped rather than reported: this directory is somebody's to tidy by hand,
/// and a stray file in it should not stop a save.
pub fn generations_in(state_dir: &Path) -> Vec<Generation> {
    let Ok(entries) = std::fs::read_dir(dir_in(state_dir)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Some(stamp) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let written_at = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Generation {
            stamp: stamp.to_string(),
            written_at,
        });
    }
    out.sort_by(|a, b| b.written_at.cmp(&a.written_at).then(b.stamp.cmp(&a.stamp)));
    out
}

/// The stamp the pointer names, or `None` when there is no pointer.
pub fn last_stamp_in(state_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(pointer_in(state_dir)).ok()?;
    let stamp = text.trim().to_string();
    (!stamp.is_empty()).then_some(stamp)
}

/// Read one generation by stamp.
pub fn load_in(state_dir: &Path, stamp: &str) -> anyhow::Result<Snapshot> {
    let file = path_in(state_dir, stamp);
    let text =
        std::fs::read_to_string(&file).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
    parse(&text).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))
}

/// Read the newest generation.
///
/// The pointer decides, and a pointer naming a file that is gone falls through
/// to the newest file actually present, since a store that lost its pointer
/// still holds everything worth restoring.
pub fn load_last_in(state_dir: &Path) -> anyhow::Result<Snapshot> {
    if let Some(stamp) = last_stamp_in(state_dir)
        && let Ok(snap) = load_in(state_dir, &stamp)
    {
        return Ok(snap);
    }
    let newest = generations_in(state_dir)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no snapshots in {}", dir_in(state_dir).display()))?;
    load_in(state_dir, &newest.stamp)
}

/// Write a snapshot as a new generation, move the pointer, then prune.
///
/// The order is the whole point. The pointer moves only after the rename has
/// returned, so a process killed at any moment leaves a store whose pointer
/// names a file that is whole. Pruning runs last, so a failure there costs disk
/// and never the snapshot just taken.
pub fn store_in(
    state_dir: &Path,
    snap: &Snapshot,
    stamp: &str,
    keep: usize,
    keep_days: u32,
    now: i64,
) -> anyhow::Result<PathBuf> {
    if snap.session.is_empty() {
        anyhow::bail!("refusing to write a snapshot with no sessions in it");
    }
    let dir = dir_in(state_dir);
    create_private_dir(&dir)?;

    let file = path_in(state_dir, stamp);
    write_private(&file, &render(snap))?;
    write_private(&pointer_in(state_dir), &format!("{stamp}\n"))?;

    for gone in prune(
        &generations_in(state_dir),
        keep,
        keep_days,
        now,
        Some(stamp),
    ) {
        let _ = std::fs::remove_file(path_in(state_dir, &gone));
        // The history goes with the generation it belongs to. Leaving it
        // behind would grow a directory nothing ever reads again.
        let _ = std::fs::remove_dir_all(history_dir_in(state_dir, &gone));
    }
    Ok(file)
}

/// Where a generation's pane history lives.
///
/// A directory of one file per pane, not an archive. tmux-companion has no tar
/// or gzip dependency and this is not worth acquiring two: 14 panes came to
/// 50 KB uncompressed against 16.7 KB gzipped, so twenty generations is a
/// megabyte either way and the difference is not a crate's worth of supply
/// chain.
pub fn history_dir_in(state_dir: &Path, stamp: &str) -> PathBuf {
    dir_in(state_dir).join(format!("{stamp}.panes"))
}

/// The file one pane's history goes in.
///
/// Named for the position that identifies a pane across a restart, since
/// `pane_id` is handed out by the running server and means nothing once it
/// exits. The separator is a dash because a session name may hold a slash.
pub fn history_file_in(
    state_dir: &Path,
    stamp: &str,
    session: &str,
    window: u32,
    pane: u32,
) -> PathBuf {
    let safe: String = session
        .chars()
        .map(|c| if c == '/' || c == '\\' { '%' } else { c })
        .collect();
    history_dir_in(state_dir, stamp).join(format!("{safe}-{window}-{pane}.txt"))
}

/// Write one pane's history, creating the generation's directory first.
pub fn store_history_in(
    state_dir: &Path,
    stamp: &str,
    session: &str,
    window: u32,
    pane: u32,
    text: &str,
) -> std::io::Result<PathBuf> {
    create_private_dir(&history_dir_in(state_dir, stamp))?;
    let file = history_file_in(state_dir, stamp, session, window, pane);
    write_private(&file, text)?;
    Ok(file)
}

/// Read one pane's history back, or nothing when it was never captured.
pub fn load_history_in(
    state_dir: &Path,
    stamp: &str,
    session: &str,
    window: u32,
    pane: u32,
) -> Option<String> {
    std::fs::read_to_string(history_file_in(state_dir, stamp, session, window, pane)).ok()
}

/// Create a directory nobody else on the machine can read.
///
/// Pane history is the text that was on somebody's screen, and the metadata
/// names every directory they work in. Neither belongs at the default umask on
/// a shared box.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    set_mode(dir, 0o700)
}

/// Write through a temporary file in the same directory and rename over the
/// target, leaving the result readable only by its owner.
///
/// `std::fs::write` truncates first and writes second, so an interruption
/// between the two leaves a half-written file where a snapshot used to be. A
/// rename within one directory is atomic on every platform this runs on.
///
/// The temporary carries the process id, so two saves racing cannot write to
/// one scratch path, and the loser still leaves a whole file behind.
fn write_private(file: &Path, contents: &str) -> std::io::Result<()> {
    let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, contents)
        .and_then(|()| set_mode(&tmp, 0o600))
        .and_then(|()| std::fs::rename(&tmp, file));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Set a path's permission bits, on the one family of platforms this runs on.
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{Header, Pane, Session, Window};

    fn made(stamp: &str, written_at: i64) -> Generation {
        Generation {
            stamp: stamp.to_string(),
            written_at,
        }
    }

    /// Twelve generations a minute apart, newest first, now = 1_000_000.
    fn twelve() -> Vec<Generation> {
        (0..12)
            .map(|i| made(&format!("g{i:02}"), 1_000_000 - i * 60))
            .collect()
    }

    fn one_session(name: &str) -> Snapshot {
        Snapshot {
            header: Header::default(),
            session: vec![Session {
                name: name.to_string(),
                path: "/w".to_string(),
                window: vec![Window {
                    index: 1,
                    name: "edit".to_string(),
                    layout: "bb62,80x20,0,0,1".to_string(),
                    active: true,
                    zoomed: false,
                    pane: vec![Pane {
                        index: 1,
                        cwd: "/w".to_string(),
                        command: "nvim".to_string(),
                        confidence: crate::saved::Confidence::Guessed,
                        active: true,
                        title: String::new(),
                    }],
                }],
            }],
        }
    }

    #[test]
    fn a_stamp_is_the_date_and_the_time_with_a_t_between_them() {
        // 2026-09-25 09:32:11 UTC.
        assert_eq!(stamp_from(1_790_328_731), "20260925T093211");
    }

    #[test]
    fn stamps_sort_in_the_order_they_happened() {
        let early = stamp_from(1_790_328_731);
        let later = stamp_from(1_790_328_791);
        assert!(early < later, "{early} should sort before {later}");
    }

    #[test]
    fn a_count_alone_drops_everything_past_it() {
        let doomed = prune(&twelve(), 5, 0, 1_000_000, None);
        assert_eq!(doomed.len(), 7);
        // Oldest first, so a caller deleting in order frees the oldest disk
        // first if it stops halfway.
        assert_eq!(doomed.first().map(String::as_str), Some("g11"));
        assert_eq!(doomed.last().map(String::as_str), Some("g05"));
    }

    #[test]
    fn an_age_alone_spares_what_the_count_would_have_taken() {
        // Every generation is inside ten minutes, so a one-day age limit keeps
        // all twelve however small the count is.
        let doomed = prune(&twelve(), 2, 1, 1_000_000, None);
        assert!(doomed.is_empty(), "{doomed:?}");
    }

    #[test]
    fn the_two_limits_together_keep_whatever_either_one_keeps() {
        // Six generations inside the age window, six well outside it, and a
        // count of three. The age window is the wider answer, so six survive.
        let mut mixed: Vec<Generation> = (0..6)
            .map(|i| made(&format!("new{i}"), 1_000_000 - i * 60))
            .collect();
        mixed.extend((0..6).map(|i| made(&format!("old{i}"), 1_000_000 - 200_000 - i * 60)));
        let doomed = prune(&mixed, 3, 1, 1_000_000, None);
        assert_eq!(doomed.len(), 6);
        assert!(doomed.iter().all(|s| s.starts_with("old")), "{doomed:?}");
    }

    #[test]
    fn the_generation_the_pointer_names_is_never_pruned() {
        // g11 is the oldest and a count of one would take it, but the pointer
        // names it, so it stays and the store keeps agreeing with itself.
        let doomed = prune(&twelve(), 1, 0, 1_000_000, Some("g11"));
        assert!(!doomed.contains(&"g11".to_string()), "{doomed:?}");
        assert_eq!(doomed.len(), 10);
    }

    #[test]
    fn nothing_to_prune_is_not_an_error() {
        assert!(prune(&[], 20, 0, 1_000_000, None).is_empty());
        assert!(prune(&twelve(), 20, 0, 1_000_000, None).is_empty());
    }

    #[test]
    fn a_snapshot_comes_back_the_way_it_went_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let snap = one_session("mysetup");
        store_in(dir.path(), &snap, "20260925T090000", 20, 0, 1_000_000).expect("store");

        assert_eq!(
            load_in(dir.path(), "20260925T090000").expect("load"),
            snap.clone()
        );
        assert_eq!(load_last_in(dir.path()).expect("load last"), snap);
        assert_eq!(
            last_stamp_in(dir.path()).as_deref(),
            Some("20260925T090000")
        );
    }

    #[test]
    fn a_capture_of_nothing_is_refused_rather_than_written() {
        let dir = tempfile::tempdir().expect("tempdir");
        let empty = Snapshot::default();
        assert!(store_in(dir.path(), &empty, "20260925T090000", 20, 0, 0).is_err());
        assert!(generations_in(dir.path()).is_empty());
    }

    #[test]
    fn the_store_and_its_pointer_are_readable_only_by_their_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = store_in(
            dir.path(),
            &one_session("mysetup"),
            "20260925T090000",
            20,
            0,
            0,
        )
        .expect("store");

        let mode = |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&pointer_in(dir.path())), 0o600);
        assert_eq!(mode(&dir_in(dir.path())), 0o700);
    }

    #[test]
    fn a_write_that_dies_before_the_rename_leaves_the_last_generation_whole() {
        // The temporary path is where a half-written file would appear, so a
        // directory sitting on it makes the write fail at exactly the point a
        // killed process would have stopped: bytes not yet renamed into place.
        let dir = tempfile::tempdir().expect("tempdir");
        let first = one_session("mysetup");
        store_in(dir.path(), &first, "20260925T090000", 20, 0, 0).expect("first store");

        let doomed = path_in(dir.path(), "20260925T091500");
        let blocked = doomed.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::create_dir(&blocked).expect("block the temporary path");

        let second = one_session("lekhani");
        assert!(store_in(dir.path(), &second, "20260925T091500", 20, 0, 0).is_err());

        // The generation that was there is untouched, the pointer still names
        // it, and the half-written one never appeared.
        assert_eq!(load_last_in(dir.path()).expect("load last"), first);
        assert_eq!(
            last_stamp_in(dir.path()).as_deref(),
            Some("20260925T090000")
        );
        assert!(!doomed.exists());
    }

    #[test]
    fn a_pointer_naming_a_file_that_went_falls_through_to_the_newest_left() {
        let dir = tempfile::tempdir().expect("tempdir");
        let snap = one_session("mysetup");
        store_in(dir.path(), &snap, "20260925T090000", 20, 0, 0).expect("store");
        std::fs::write(pointer_in(dir.path()), "20260925T999999\n").expect("bend the pointer");

        assert_eq!(load_last_in(dir.path()).expect("load last"), snap);
    }

    #[test]
    fn storing_prunes_what_the_limits_no_longer_allow() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..4 {
            let stamp = format!("2026092{i}T090000");
            store_in(dir.path(), &one_session("mysetup"), &stamp, 2, 0, 1_000_000).expect("store");
        }
        let left: Vec<String> = generations_in(dir.path())
            .into_iter()
            .map(|g| g.stamp)
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
        assert!(left.contains(&"20260923T090000".to_string()), "{left:?}");
    }

    #[test]
    fn a_stray_file_in_the_directory_does_not_stop_a_save() {
        let dir = tempfile::tempdir().expect("tempdir");
        store_in(
            dir.path(),
            &one_session("mysetup"),
            "20260925T090000",
            20,
            0,
            0,
        )
        .expect("store");
        std::fs::write(dir_in(dir.path()).join("notes.txt"), "mine").expect("stray file");

        assert!(
            store_in(
                dir.path(),
                &one_session("mysetup"),
                "20260925T091500",
                20,
                0,
                0
            )
            .is_ok()
        );
        assert_eq!(generations_in(dir.path()).len(), 2);
    }

    #[test]
    fn pane_history_is_written_per_pane_and_read_back_by_position() {
        let dir = tempfile::tempdir().expect("tempdir");
        store_history_in(dir.path(), "20260925T090000", "mysetup", 2, 1, "hello\n")
            .expect("store history");
        assert_eq!(
            load_history_in(dir.path(), "20260925T090000", "mysetup", 2, 1).as_deref(),
            Some("hello\n")
        );
        // A pane nothing was captured for reads back as nothing rather than as
        // an error, because history is the optional half.
        assert_eq!(
            load_history_in(dir.path(), "20260925T090000", "mysetup", 9, 9),
            None
        );
    }

    #[test]
    fn a_session_name_with_a_slash_does_not_become_a_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = store_history_in(dir.path(), "20260925T090000", "w/api", 1, 1, "x")
            .expect("store history");
        assert_eq!(
            file.parent(),
            Some(history_dir_in(dir.path(), "20260925T090000").as_path())
        );
        assert_eq!(
            load_history_in(dir.path(), "20260925T090000", "w/api", 1, 1).as_deref(),
            Some("x")
        );
    }

    #[test]
    fn history_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = store_history_in(dir.path(), "20260925T090000", "mysetup", 1, 1, "secret")
            .expect("store history");
        let mode = |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&history_dir_in(dir.path(), "20260925T090000")), 0o700);
    }

    #[test]
    fn pruning_a_generation_takes_its_history_with_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..3 {
            let stamp = format!("2026092{i}T090000");
            store_history_in(dir.path(), &stamp, "mysetup", 1, 1, "old").expect("history");
            store_in(dir.path(), &one_session("mysetup"), &stamp, 1, 0, 1_000_000).expect("store");
        }
        assert!(!history_dir_in(dir.path(), "20260920T090000").exists());
        assert!(history_dir_in(dir.path(), "20260922T090000").exists());
    }

    #[test]
    fn no_snapshots_at_all_is_an_error_naming_the_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = load_last_in(dir.path()).expect_err("nothing to load");
        assert!(err.to_string().contains("sessions"), "{err}");
    }
}
