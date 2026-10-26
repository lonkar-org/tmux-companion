//! Health notes: reasons on the health mark that a tool outside the daemon
//! put there, `health note SOURCE TEXT`, one per source.
//!
//! Every other reason is the daemon's own finding: a failed timer, a stale
//! config, quiet hours. A note is somebody else's, a dotfiles check saying
//! `dotfiles: 3 drifted`, so it is said in the source's name and lives until
//! the source clears it, replaces it, or lets its `--ttl` run out. It is
//! kept in `health-notes.json` in the state directory and read back when
//! the daemon starts, so a restart doesn't drop what the source said once a
//! day. `health ack` leaves notes standing: the source will say it again,
//! and only the source knows when it stops being true.
//!
//! A note is a line in `health`, in `brief` and behind the bar's health
//! mark, read top to bottom like every other reason.

use serde::{Deserialize, Serialize};

/// The file the notes live in, in the state directory.
pub const FILE: &str = "health-notes.json";

/// The longest a note's text may be, in characters; a longer one is cut
/// with an ellipsis, since it is a line on the mark and not a report.
pub const TEXT_MAX: usize = 120;

/// The longest a source's name may be.
pub const SOURCE_MAX: usize = 32;

/// One source's note.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// Who said it: letters, digits, `-`, `_` and `.`.
    pub source: String,
    /// What it said, one line.
    pub text: String,
    /// Unix seconds it was set.
    pub set_at: u64,
    /// Unix seconds it stops being said, for a source that may stop
    /// refreshing it; `None` lasts until cleared.
    #[serde(default)]
    pub expires_at: Option<u64>,
}

impl Note {
    /// Whether it is still said at `now`.
    pub fn live(&self, now: u64) -> bool {
        self.expires_at.is_none_or(|t| now < t)
    }

    /// The reason as the mark says it: `dotfiles: 3 drifted`, the source's
    /// name first unless the text already starts with it.
    pub fn reason(&self) -> String {
        if self
            .text
            .strip_prefix(&self.source)
            .is_some_and(|rest| rest.starts_with(':'))
        {
            self.text.clone()
        } else {
            format!("{}: {}", self.source, self.text)
        }
    }
}

/// Whether a name can be a source's: it is the key a note is replaced and
/// cleared by, and the word the mark says it in.
pub fn check_source(source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Err("a note needs a source, the name it is said in".to_string());
    }
    if source.chars().count() > SOURCE_MAX {
        return Err(format!(
            "`{source}` is longer than {SOURCE_MAX} characters, the most a source may be"
        ));
    }
    if let Some(bad) = source
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
    {
        return Err(format!(
            "`{bad}` can't be in a source; use letters, digits, - _ and ."
        ));
    }
    Ok(())
}

/// A note's text as one line: control characters and runs of whitespace
/// become one space, the ends are trimmed, and a text longer than
/// [`TEXT_MAX`] is cut with `…`. Empty is refused: clearing is `--clear`.
pub fn clean_text(text: &str) -> Result<String, String> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    let line = words.join(" ");
    if line.is_empty() {
        return Err("a note needs text; --clear takes one away".to_string());
    }
    if line.chars().count() > TEXT_MAX {
        let cut: String = line.chars().take(TEXT_MAX - 1).collect();
        return Ok(format!("{}…", cut.trim_end()));
    }
    Ok(line)
}

/// Set `note`, replacing its source's note in place, so a source that
/// refreshes keeps its place among the reasons.
pub fn set(notes: &mut Vec<Note>, note: Note) {
    match notes.iter_mut().find(|n| n.source == note.source) {
        Some(n) => *n = note,
        None => notes.push(note),
    }
}

/// Take `source`'s note away; whether it had one.
pub fn clear(notes: &mut Vec<Note>, source: &str) -> bool {
    let before = notes.len();
    notes.retain(|n| n.source != source);
    notes.len() != before
}

/// The reasons the notes still live at `now` give, in the order they were
/// first set.
pub fn reasons(notes: &[Note], now: u64) -> Vec<String> {
    notes
        .iter()
        .filter(|n| n.live(now))
        .map(Note::reason)
        .collect()
}

/// What `health note` asked for, done to `notes`: the line saying what
/// happened, and whether the notes changed and so want writing.
pub fn answer(
    args: &crate::proto::HealthNoteArgs,
    notes: &mut Vec<Note>,
    now: u64,
) -> Result<(String, bool), String> {
    check_source(&args.source)?;
    if args.clear {
        let had = clear(notes, &args.source);
        let line = if had {
            format!("cleared {}'s note", args.source)
        } else {
            format!("{} had no note", args.source)
        };
        return Ok((line, had));
    }
    let text = clean_text(args.text.as_deref().unwrap_or(""))?;
    let note = Note {
        source: args.source.clone(),
        text,
        set_at: now,
        expires_at: args.ttl_secs.map(|s| now.saturating_add(s.max(1))),
    };
    let mut line = format!("noted {}", note.reason());
    if let Some(secs) = args.ttl_secs {
        line.push_str(&format!(", for {} unless said again", span(secs.max(1))));
    }
    set(notes, note);
    Ok((line, true))
}

/// A TTL in the largest unit that divides it evenly, `90m`, `2d`, `45s`,
/// so the line says the TTL that was set: `age` would round 90m down to
/// `1h`, which reads as a different TTL from the one kept.
pub fn span(secs: u64) -> String {
    for (unit, size) in [("d", 86_400), ("h", 3600), ("m", 60)] {
        if secs >= size && secs.is_multiple_of(size) {
            return format!("{}{unit}", secs / size);
        }
    }
    format!("{secs}s")
}

/// The notes file in a state directory.
pub fn path_in(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join(FILE)
}

/// The notes a file holds, the expired ones dropped. A file that is missing
/// or doesn't parse holds none, and a note whose source or text wouldn't be
/// accepted now is left out, since the file is a place anything can write.
pub fn read(path: &std::path::Path, now: u64) -> Vec<Note> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(notes) = serde_json::from_str::<Vec<Note>>(&text) else {
        return Vec::new();
    };
    notes
        .into_iter()
        .filter(|n| n.live(now) && check_source(&n.source).is_ok())
        .filter_map(|mut n| {
            n.text = clean_text(&n.text).ok()?;
            Some(n)
        })
        .collect()
}

/// Write the notes, through a file beside it renamed into place, so a
/// reader never sees half of one. No notes removes the file.
pub fn write(path: &std::path::Path, notes: &[Note]) -> std::io::Result<()> {
    if notes.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(notes)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::HealthNoteArgs;

    fn args(source: &str, text: Option<&str>, clear: bool, ttl: Option<u64>) -> HealthNoteArgs {
        HealthNoteArgs {
            source: source.into(),
            text: text.map(String::from),
            clear,
            ttl_secs: ttl,
        }
    }

    #[test]
    fn a_note_is_said_in_its_sources_name_once() {
        let n = |text: &str| Note {
            source: "dotfiles".into(),
            text: text.into(),
            set_at: 0,
            expires_at: None,
        };
        assert_eq!(n("3 drifted").reason(), "dotfiles: 3 drifted");
        assert_eq!(n("dotfiles: 3 drifted").reason(), "dotfiles: 3 drifted");
        assert_eq!(n("dotfilesx drifted").reason(), "dotfiles: dotfilesx drifted");
    }

    #[test]
    fn a_source_refreshing_replaces_its_note_in_place() {
        let mut notes = Vec::new();
        answer(&args("a", Some("one"), false, None), &mut notes, 10).unwrap();
        answer(&args("b", Some("two"), false, None), &mut notes, 10).unwrap();
        let (line, changed) =
            answer(&args("a", Some("three"), false, None), &mut notes, 20).unwrap();
        assert_eq!(line, "noted a: three");
        assert!(changed);
        assert_eq!(reasons(&notes, 20), ["a: three", "b: two"]);
        let (line, changed) = answer(&args("a", None, true, None), &mut notes, 20).unwrap();
        assert_eq!((line.as_str(), changed), ("cleared a's note", true));
        let (line, changed) = answer(&args("a", None, true, None), &mut notes, 20).unwrap();
        assert_eq!((line.as_str(), changed), ("a had no note", false));
    }

    #[test]
    fn a_note_with_a_ttl_stops_being_said_when_it_runs_out() {
        let mut notes = Vec::new();
        let (line, _) =
            answer(&args("ci", Some("red"), false, Some(3600)), &mut notes, 100).unwrap();
        assert_eq!(line, "noted ci: red, for 1h unless said again");
        assert_eq!(reasons(&notes, 3699), ["ci: red"]);
        assert!(reasons(&notes, 3700).is_empty());
    }

    #[test]
    fn a_ttl_is_said_as_it_was_set() {
        assert_eq!(span(5400), "90m");
        assert_eq!(span(3600), "1h");
        assert_eq!(span(172_800), "2d");
        assert_eq!(span(129_600), "36h");
        assert_eq!(span(45), "45s");
        assert_eq!(span(3601), "3601s");
        let mut notes = Vec::new();
        let (line, _) = answer(&args("p", Some("x"), false, Some(5400)), &mut notes, 0).unwrap();
        assert_eq!(line, "noted p: x, for 90m unless said again");
        assert_eq!(notes[0].expires_at, Some(5400), "kept exact, not rounded");
    }

    #[test]
    fn sources_and_texts_are_checked() {
        assert!(check_source("dotfiles").is_ok());
        assert!(check_source("dots.v4_x-1").is_ok());
        assert!(check_source("").is_err());
        assert!(check_source("a b").is_err());
        assert!(check_source("a;b").is_err());
        assert!(check_source(&"a".repeat(33)).is_err());
        assert_eq!(
            clean_text("  3\tdrifted\n\x1b[31m ").unwrap(),
            "3 drifted [31m"
        );
        assert!(clean_text(" \n ").is_err());
        let long = clean_text(&"x".repeat(200)).unwrap();
        assert_eq!(long.chars().count(), TEXT_MAX);
        assert!(long.ends_with('…'));
        let mut notes = Vec::new();
        assert!(answer(&args("ok", None, false, None), &mut notes, 0).is_err());
        assert!(answer(&args("no way", Some("x"), false, None), &mut notes, 0).is_err());
    }

    #[test]
    fn notes_survive_a_write_and_a_read_and_expired_ones_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_in(dir.path());
        let notes = vec![
            Note {
                source: "a".into(),
                text: "kept".into(),
                set_at: 1,
                expires_at: None,
            },
            Note {
                source: "b".into(),
                text: "gone".into(),
                set_at: 1,
                expires_at: Some(50),
            },
        ];
        write(&path, &notes).unwrap();
        assert_eq!(read(&path, 10), notes);
        assert_eq!(read(&path, 60), notes[..1]);
        write(&path, &[]).unwrap();
        assert!(!path.exists());
        write(&path, &[]).unwrap();
        std::fs::write(&path, "not json").unwrap();
        assert!(read(&path, 0).is_empty());
    }
}
