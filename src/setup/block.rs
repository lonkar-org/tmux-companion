//! The part of tmux.conf `setup` owns: a fenced block, and the write that puts
//! it on disk.
//!
//! Everything outside the fences is somebody's own and is never touched. The
//! block says which items it holds, one header line each with the key chosen,
//! so it can be rebuilt from itself: adding an item reads the entries back,
//! adds one, and writes every entry again from the catalog. An entry whose id
//! the catalog no longer knows keeps the lines it had.
//!
//! The write goes through a symlink rather than over it. tmux.conf is a link
//! into a dotfiles checkout on more machines than not, and replacing the link
//! with a regular file cuts it off from the checkout without a word, which is
//! how a config was lost on 2026-09-26.

use std::path::{Path, PathBuf};

use super::catalog::Item;

/// What the fence lines start with. Matched on this, never on the words after
/// it, so rewording the fences does not orphan a block already on disk.
pub const BEGIN: &str = "# >>> tmux-companion setup";
/// The closing fence's start.
pub const END: &str = "# <<< tmux-companion setup";
/// What an entry's header line starts with; the id and the key follow.
pub const ENTRY: &str = "# setup-item:";

// @Yogesh(word): the words after the opening fence in tmux.conf
const BEGIN_WORDS: &str =
    ">>> written by tmux-companion setup; lines between the fences are replaced";
// @Yogesh(word): the words after the closing fence in tmux.conf
const END_WORDS: &str = "<<<";

/// One item in the block: its id, the key it was bound on, and its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The catalog id.
    pub id: String,
    /// The key chosen, for a binding.
    pub key: Option<String>,
    /// The lines under the header, kept for an id the catalog has lost.
    pub body: Vec<String>,
}

/// Where the block is in a file, as line indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    begin: usize,
    end: usize,
}

fn span(lines: &[&str]) -> Option<Span> {
    let begin = lines.iter().position(|l| l.starts_with(BEGIN))?;
    let end = lines[begin..].iter().position(|l| l.starts_with(END))? + begin;
    Some(Span { begin, end })
}

/// The entries in a file's block, empty when it has none.
pub fn entries(text: &str) -> Vec<Entry> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(Span { begin, end }) = span(&lines) else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = Vec::new();
    for line in &lines[begin + 1..end] {
        if let Some(rest) = line.strip_prefix(ENTRY) {
            let mut words = rest.split_whitespace();
            let Some(id) = words.next() else { continue };
            out.push(Entry {
                id: id.to_string(),
                key: words.next().map(str::to_string),
                body: Vec::new(),
            });
        } else if let Some(last) = out.last_mut() {
            last.body.push((*line).to_string());
        }
    }
    out
}

/// The block's lines, fences included, for these entries.
pub fn render(entries: &[Entry], catalog: &[Item]) -> Vec<String> {
    let mut out = vec![format!("{BEGIN} {BEGIN_WORDS}")];
    for e in entries {
        match &e.key {
            Some(k) => out.push(format!("{ENTRY} {} {k}", e.id)),
            None => out.push(format!("{ENTRY} {}", e.id)),
        }
        match catalog.iter().find(|i| i.id == e.id) {
            Some(item) => out.extend(
                item.snippet_for(e.key.as_deref())
                    .lines()
                    .map(|l| l.trim_end().to_string()),
            ),
            None => out.extend(e.body.iter().cloned()),
        }
    }
    out.push(format!("{END} {END_WORDS}"));
    out
}

/// Put an entry in the list: in place of one with the same id, else at the end.
pub fn with(mut entries: Vec<Entry>, entry: Entry) -> Vec<Entry> {
    match entries.iter_mut().find(|e| e.id == entry.id) {
        Some(e) => *e = entry,
        None => entries.push(entry),
    }
    entries
}

/// Whether a line loads tpm, which has to stay the last thing in the file.
fn runs_tpm(line: &str) -> bool {
    let t = line.trim_start();
    (t.starts_with("run ") || t.starts_with("run-shell ")) && t.contains("tpm/tpm")
}

/// The file with its block holding these entries.
///
/// A block already there is rewritten between its fences and nowhere else. A
/// file without one gets it directly above the line that runs tpm, because
/// tpm's README asks for that line to be the last, and otherwise at the end.
pub fn apply(text: &str, entries: &[Entry], catalog: &[Item]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let block = render(entries, catalog);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + block.len() + 1);
    if let Some(Span { begin, end }) = span(&lines) {
        out.extend(lines[..begin].iter().map(|l| (*l).to_string()));
        out.extend(block);
        out.extend(lines[end + 1..].iter().map(|l| (*l).to_string()));
    } else if let Some(at) = lines.iter().position(|l| runs_tpm(l)) {
        out.extend(lines[..at].iter().map(|l| (*l).to_string()));
        out.extend(block);
        out.push(String::new());
        out.extend(lines[at..].iter().map(|l| (*l).to_string()));
    } else {
        out.extend(lines.iter().map(|l| (*l).to_string()));
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.extend(block);
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// The file a write lands in: the end of any chain of symlinks.
///
/// A path that does not exist yet is its own target, and so is a dangling
/// link's last hop, so the first write through a fresh link creates the
/// checkout's file rather than a regular file where the link was.
pub fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    let mut here = path.to_path_buf();
    for _ in 0..40 {
        match std::fs::read_link(&here) {
            Ok(next) => {
                here = match here.parent() {
                    Some(dir) if next.is_relative() => dir.join(next),
                    _ => next,
                };
            }
            Err(_) => break,
        }
    }
    here
}

/// Write a file through any symlink, atomically.
///
/// The new text goes to a temporary file in the target's own directory, so
/// the rename is on one filesystem and nobody ever reads half a config, and
/// then onto the resolved target. The link, if there is one, is left pointing
/// where it pointed. The target's permissions are kept.
pub fn write_through(path: &Path, text: &str) -> std::io::Result<PathBuf> {
    let target = resolve(path);
    let dir = target
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let tmp = dir.join(format!(".{name}.tc-setup.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, text).and_then(|()| {
        if let Ok(meta) = std::fs::metadata(&target) {
            std::fs::set_permissions(&tmp, meta.permissions())?;
        }
        std::fs::rename(&tmp, &target)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::catalog::items;

    fn entry(id: &str, key: Option<&str>) -> Entry {
        Entry {
            id: id.into(),
            key: key.map(str::to_string),
            body: Vec::new(),
        }
    }

    #[test]
    fn a_file_without_a_block_gets_one_at_the_end_and_keeps_every_line() {
        let before = "set -g mouse on\n# my own comment\nbind x kill-pane\n";
        let after = apply(before, &[entry("panes", Some("g"))], &items());
        assert!(after.starts_with(before), "{after}");
        let tail: Vec<&str> = after.lines().skip(3).collect();
        assert_eq!(tail[0], "");
        assert!(tail[1].starts_with(BEGIN), "{after}");
        assert_eq!(tail[2], "# setup-item: panes g");
        assert!(tail[3].contains("tmux-companion panes"), "{after}");
        assert!(tail[4].starts_with(END), "{after}");
    }

    #[test]
    fn the_block_goes_above_the_line_that_runs_tpm() {
        let before = "set -g @plugin 'tmux-plugins/tpm'\n\nrun '~/.tmux/plugins/tpm/tpm'\n";
        let after = apply(before, &[entry("inbox", Some("M-g"))], &items());
        let lines: Vec<&str> = after.lines().collect();
        let begin = lines.iter().position(|l| l.starts_with(BEGIN)).unwrap();
        let end = lines.iter().position(|l| l.starts_with(END)).unwrap();
        let tpm = lines.iter().position(|l| l.contains("tpm/tpm")).unwrap();
        assert!(begin < end && end < tpm, "{after}");
        assert_eq!(lines.last(), Some(&"run '~/.tmux/plugins/tpm/tpm'"));
        // `run-shell` is the same line spelled out.
        let spelled = "run-shell ~/.tmux/plugins/tpm/tpm\n";
        let after = apply(spelled, &[entry("inbox", Some("M-g"))], &items());
        assert!(after.ends_with(spelled), "{after}");
    }

    #[test]
    fn a_block_already_there_is_rewritten_and_nothing_around_it_moves() {
        let catalog = items();
        let first = apply("above\n", &[entry("panes", Some("g"))], &catalog);
        let with_below = format!("{first}below one\nbelow two\n");
        let got = entries(&with_below);
        assert_eq!(got.len(), 1);
        let more = with(got, entry("inbox", Some("M-g")));
        let after = apply(&with_below, &more, &catalog);
        assert!(after.starts_with("above\n"), "{after}");
        assert!(after.ends_with("below one\nbelow two\n"), "{after}");
        assert_eq!(after.matches(BEGIN).count(), 1, "{after}");
        let ids: Vec<String> = entries(&after).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["panes", "inbox"]);
    }

    #[test]
    fn a_new_key_replaces_the_entry_rather_than_adding_a_second() {
        let catalog = items();
        let once = apply("", &[entry("panes", Some("g"))], &catalog);
        let again = with(entries(&once), entry("panes", Some("j")));
        let after = apply(&once, &again, &catalog);
        let got: Vec<(String, Option<String>)> =
            entries(&after).into_iter().map(|e| (e.id, e.key)).collect();
        assert_eq!(got, vec![("panes".to_string(), Some("j".to_string()))]);
        assert!(after.contains("\" j display-popup"), "{after}");
        assert!(!after.contains("\" g display-popup"), "{after}");
    }

    #[test]
    fn an_entry_the_catalog_lost_keeps_its_lines() {
        let text = format!(
            "{BEGIN} x\n{ENTRY} gone-now\nbind q something-old\n{ENTRY} panes g\nbind g old\n{END} y\n"
        );
        let got = entries(&text);
        assert_eq!(got[0].body, vec!["bind q something-old".to_string()]);
        let after = apply(&text, &got, &items());
        assert!(after.contains("bind q something-old"), "{after}");
        // A known id is rendered again from the catalog, not from its body.
        assert!(!after.contains("bind g old"), "{after}");
        assert!(after.contains("tmux-companion panes"), "{after}");
    }

    #[test]
    fn the_fences_are_found_by_their_start_whatever_words_follow() {
        let text = format!("{BEGIN} some older wording\n{ENTRY} panes g\n{END}\n");
        assert_eq!(entries(&text).len(), 1);
    }

    #[test]
    fn a_write_goes_through_the_link_and_leaves_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = dir.path().join("dotfiles");
        std::fs::create_dir_all(&checkout).unwrap();
        let real = checkout.join("tmux.conf");
        std::fs::write(&real, "old\n").unwrap();
        let link = dir.path().join(".tmux.conf");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let landed = write_through(&link, "new\n").unwrap();
        assert_eq!(landed, std::fs::canonicalize(&real).unwrap());
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link was replaced"
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new\n");
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "new\n");
        // No temporary file is left behind in the target's directory.
        let left: Vec<_> = std::fs::read_dir(&checkout).unwrap().collect();
        assert_eq!(left.len(), 1);
    }

    #[test]
    fn a_link_to_a_file_not_there_yet_creates_the_file_it_points_at() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("checkout/tmux.conf");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        let link = dir.path().join(".tmux.conf");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_through(&link, "made\n").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "made\n");
    }

    #[test]
    fn a_write_keeps_the_files_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tmux.conf");
        std::fs::write(&file, "x\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_through(&file, "y\n").unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
