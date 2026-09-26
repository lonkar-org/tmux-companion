//! Snapshots of the whole tmux server, and the generations they are kept in.
//!
//! A saved layout in [`crate::saved`] answers "what does this project look
//! like": one file per directory, overwritten on demand, edited by hand, kept
//! for good. This answers "what was I doing at 09:23": one file per capture,
//! kept in generations, thrown away oldest first.
//!
//! The two are separate stores on purpose. Merging them loses a session that
//! has no project to be keyed on, which is every scratch session somebody
//! named by hand, and it gives a file people edit the lifetime of a file
//! nothing should outlive its usefulness.
//!
//! Nothing here talks to tmux. Capture fills these types in from what tmux
//! reported and restore reads them back, so everything in this module is
//! arithmetic over data somebody else fetched, which is what lets it be tested
//! without a server. The one exception is `idle`, which is a listing rather
//! than a snapshot and asks tmux itself, with the parse kept pure.

pub mod capture;
pub mod cli;
pub mod idle;
pub mod import;
pub mod restore;
pub mod store;
pub mod summary;
pub mod timer;

use serde::{Deserialize, Serialize};

use crate::saved::Confidence;

/// The version this crate writes into every snapshot it creates.
///
/// A store that keeps twenty generations outlives more than one change to its
/// own format, and a change nothing can detect is a change that reads last
/// month's files as garbage. A reader that finds a number it does not know
/// refuses the file rather than guessing at it.
pub const FORMAT: u32 = 1;

/// Everything one capture of the server produced.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// Where this came from and what took it.
    pub header: Header,
    /// The sessions, in the order tmux listed them.
    #[serde(default)]
    pub session: Vec<Session>,
}

/// Provenance: enough to say what a generation is before restoring it.
///
/// A restore that says "from 09:23, forty seconds before the crash" is a
/// different offer from one that says "restore?", and the difference is
/// entirely in what this struct carries.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// [`FORMAT`] as it was when this was written.
    pub format: u32,
    /// When, as [`crate::tasks::format_unix`] writes it.
    #[serde(default)]
    pub captured_at: String,
    /// Whether the daemon that wrote this went on to shut down cleanly.
    ///
    /// Written `true` by a capture the person asked for and by the one
    /// `sessions shutdown` takes on its way out. A timer's capture writes
    /// `false`, because the daemon cannot know yet, and nothing rewrites it
    /// afterwards: a snapshot that turned out to be the last one before a
    /// crash is exactly the one nobody was there to correct.
    #[serde(default)]
    pub clean: bool,
    /// What `tmux -V` said, since a layout string is read by that version.
    #[serde(default)]
    pub tmux_version: String,
    /// The build that wrote it, as `doctor` reports it.
    #[serde(default)]
    pub companion_version: String,
    /// The machine, because these dotfiles are synced between several and a
    /// snapshot names directories that may exist on only one of them.
    #[serde(default)]
    pub hostname: String,
    /// The session that was attached when this was taken, so a restore knows
    /// which one to land on.
    #[serde(default)]
    pub attached: String,
    /// Where this came from, when it was not a capture of ours.
    #[serde(default)]
    pub imported_from: String,
}

impl Default for Header {
    fn default() -> Self {
        Self {
            format: FORMAT,
            captured_at: String::new(),
            clean: false,
            tmux_version: String::new(),
            companion_version: String::new(),
            hostname: String::new(),
            attached: String::new(),
            imported_from: String::new(),
        }
    }
}

/// One session, by the name it had.
///
/// The name is kept verbatim rather than derived from the directory, which is
/// the whole reason this store exists next to the per-project one: a session
/// called `y` holding one bare shell has no project and still has to come
/// back.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Session {
    /// `#{session_name}`.
    pub name: String,
    /// `#{session_path}`, which is where a new window in it would open.
    #[serde(default)]
    pub path: String,
    /// The windows, in index order.
    #[serde(default)]
    pub window: Vec<Window>,
}

/// One window, and the shape its panes were in.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// `#{window_index}`.
    pub index: u32,
    /// `#{window_name}`.
    #[serde(default)]
    pub name: String,
    /// `#{window_layout}`, tmux's own description of the pane geometry.
    #[serde(default)]
    pub layout: String,
    /// `#{window_active}`: the window this session was on.
    #[serde(default)]
    pub active: bool,
    /// `#{window_zoomed_flag}`, restored after the layout is applied because
    /// zooming first is a shape the layout then undoes.
    #[serde(default)]
    pub zoomed: bool,
    /// The panes, in index order.
    #[serde(default)]
    pub pane: Vec<Pane>,
}

/// One pane: where it was, what it was running, and how sure the capture is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Pane {
    /// `#{pane_index}`.
    pub index: u32,
    /// `#{pane_current_path}`.
    #[serde(default)]
    pub cwd: String,
    /// What to run here, empty for a pane that was sitting at a prompt.
    #[serde(default)]
    pub command: String,
    /// Whether [`Pane::command`] is what the pane was started with, what it
    /// happened to be running, or nothing at all.
    #[serde(default)]
    pub confidence: Confidence,
    /// `#{pane_active}`: the pane this window was on.
    #[serde(default)]
    pub active: bool,
}

impl Snapshot {
    /// How many panes this holds, across every session.
    pub fn pane_count(&self) -> usize {
        self.session
            .iter()
            .flat_map(|s| &s.window)
            .map(|w| w.pane.len())
            .sum()
    }

    /// How many windows this holds, across every session.
    pub fn window_count(&self) -> usize {
        self.session.iter().map(|s| s.window.len()).sum()
    }

    /// The session a restore should attach to, from the header, falling back to
    /// the first one captured.
    ///
    /// `None` only when the snapshot holds no sessions at all, which a capture
    /// refuses to write and an imported file might still contain.
    pub fn attach_target(&self) -> Option<&str> {
        let named = self
            .session
            .iter()
            .find(|s| s.name == self.header.attached)
            .map(|s| s.name.as_str());
        named.or_else(|| self.session.first().map(|s| s.name.as_str()))
    }

    /// The build that wrote this, when it is newer than the one reading it.
    ///
    /// The format number says whether the file can be read at all; this says
    /// whether the build that wrote it knew something this one does not, which
    /// is a warning rather than a refusal. `None` when the writer is this
    /// build, an older one, or one whose version carries no stamp.
    pub fn written_by_newer_build(&self) -> Option<&str> {
        let theirs = self.header.companion_version.as_str();
        built_after(theirs, &crate::proto::build_id()).then_some(theirs)
    }
}

/// Render a snapshot as the TOML that goes on disk.
pub fn render(snap: &Snapshot) -> String {
    let body = toml::to_string_pretty(snap).unwrap_or_default();
    format!(
        "# Written by tmux-companion. This is one moment, not a setting:\n\
         # it is replaced by the next capture and dropped when it ages out of\n\
         # the generations `[sessions] keep` allows. Edit the per-project\n\
         # layouts under projects/ instead, which nothing overwrites on a timer.\n\
         \n{body}"
    )
}

/// The one field a format check needs, read before anything else is.
///
/// Without `deny_unknown_fields`, on purpose: a file from a build that added a
/// key is exactly the file this has to be able to read the number out of.
/// Strict parsing first would refuse it as "unknown field `whatever`", which
/// names the symptom and hides the cause.
#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    header: ProbeHeader,
}

/// The header half of [`Probe`].
#[derive(Deserialize, Default)]
struct ProbeHeader {
    #[serde(default)]
    format: u32,
}

/// Parse a snapshot, refusing a format this build does not know.
///
/// The refusal names both numbers, because the person reading it has a
/// directory full of files and needs to know which ones this binary can still
/// read. A newer number gets told which way round the mismatch is, since the
/// fix is to upgrade rather than to delete the file.
pub fn parse(text: &str) -> anyhow::Result<Snapshot> {
    let probe: Probe = toml::from_str(text)?;
    if probe.header.format > FORMAT {
        anyhow::bail!(
            "snapshot format {}, this build reads {}; a newer tmux-companion wrote it",
            probe.header.format,
            FORMAT
        );
    }
    let snap: Snapshot = toml::from_str(text)?;
    if snap.header.format != FORMAT {
        anyhow::bail!(
            "snapshot format {}, but this build reads {}",
            snap.header.format,
            FORMAT
        );
    }
    Ok(snap)
}

/// The digits after `+` in a build id, which is when the build was made.
///
/// Stops at the first character that is not a digit, so a build id that goes
/// on to name a commit (`0.2.0+1790400979.abc1234`) still reads as the
/// seconds, and one with no stamp at all reads as nothing.
pub fn build_stamp(build_id: &str) -> Option<u64> {
    let (_, rest) = build_id.split_once('+')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Whether `theirs` was built after `mine`, by stamp.
///
/// Numerically rather than by string, so `+999` does not sort above
/// `+1790400979`, and false when either side has no stamp to compare: a
/// version nobody can place is not evidence of anything.
pub fn built_after(theirs: &str, mine: &str) -> bool {
    match (build_stamp(theirs), build_stamp(mine)) {
        (Some(t), Some(m)) => t > m,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A snapshot the shape of this laptop: 7 sessions, 14 windows, 14 panes,
    /// two of them agents carrying a resume id.
    fn laptop() -> Snapshot {
        let project = |name: &str, path: &str, agent: &str| Session {
            name: name.to_string(),
            path: path.to_string(),
            window: vec![
                Window {
                    index: 1,
                    name: "edit".to_string(),
                    layout: "bb62,272x67,0,0,1".to_string(),
                    active: true,
                    zoomed: false,
                    pane: vec![Pane {
                        index: 1,
                        cwd: path.to_string(),
                        command: "nvim".to_string(),
                        confidence: Confidence::Guessed,
                        active: true,
                    }],
                },
                Window {
                    index: 2,
                    name: "ai".to_string(),
                    layout: "bb62,272x67,0,0,2".to_string(),
                    active: false,
                    zoomed: false,
                    pane: vec![Pane {
                        index: 1,
                        cwd: path.to_string(),
                        command: agent.to_string(),
                        confidence: Confidence::Guessed,
                        active: true,
                    }],
                },
            ],
        };
        Snapshot {
            header: Header {
                format: FORMAT,
                captured_at: "2026-09-25 09:23:16 UTC".to_string(),
                clean: true,
                tmux_version: "tmux 3.7c".to_string(),
                companion_version: "0.2.0+1790261531".to_string(),
                hostname: "laptop".to_string(),
                attached: "tmux-companion".to_string(),
                imported_from: String::new(),
            },
            session: vec![
                project(
                    "icf-c_com",
                    "/Users/yogesh/lonkar-org/icf-c.com",
                    "claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5",
                ),
                project(
                    "lekhani",
                    "/Users/yogesh/lekhani",
                    "claude --resume fe655371-f2f3-4d81-b635-28245507b7c8",
                ),
                {
                    // Three windows, because this one really does carry a
                    // second editor, and a fixture that rounds the awkward
                    // session off is a fixture that never finds anything.
                    let mut three = project(
                        "lonkar_org",
                        "/Users/yogesh/lonkar-org/lonkar.org",
                        "claude",
                    );
                    three.window.push(Window {
                        index: 3,
                        name: "edit".to_string(),
                        layout: "bb62,272x67,0,0,3".to_string(),
                        active: false,
                        zoomed: true,
                        pane: vec![Pane {
                            index: 1,
                            cwd: "/Users/yogesh/lonkar-org/lonkar.org".to_string(),
                            command: "nvim".to_string(),
                            confidence: Confidence::Guessed,
                            active: true,
                        }],
                    });
                    three
                },
                project("mysetup", "/Users/yogesh/git-repos/mysetup", "claude"),
                project(
                    "tmux-companion",
                    "/Users/yogesh/lonkar-org/tmux-companion",
                    "claude",
                ),
                project(
                    "yogesh_lonkar_org",
                    "/Users/yogesh/lonkar-org/yogesh.lonkar.org",
                    "claude",
                ),
                Session {
                    name: "y".to_string(),
                    path: "/Users/yogesh".to_string(),
                    window: vec![Window {
                        index: 1,
                        name: "zsh".to_string(),
                        layout: "bb62,272x67,0,0,13".to_string(),
                        active: true,
                        zoomed: false,
                        pane: vec![Pane {
                            index: 1,
                            cwd: "/Users/yogesh".to_string(),
                            command: String::new(),
                            confidence: Confidence::Shell,
                            active: true,
                        }],
                    }],
                },
            ],
        }
    }

    #[test]
    fn a_whole_laptop_survives_the_round_trip() {
        let snap = laptop();
        assert_eq!(snap.session.len(), 7);
        assert_eq!(snap.window_count(), 14);
        assert_eq!(snap.pane_count(), 14);

        let back = parse(&render(&snap)).expect("parse");
        assert_eq!(back, snap);
    }

    #[test]
    fn the_resume_id_comes_back_byte_for_byte() {
        // The whole feature is this string surviving a reboot, so it gets a
        // test of its own rather than riding on the equality above.
        let back = parse(&render(&laptop())).expect("parse");
        assert_eq!(
            back.session[0].window[1].pane[0].command,
            "claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5"
        );
    }

    #[test]
    fn a_session_with_no_project_keeps_its_name() {
        let back = parse(&render(&laptop())).expect("parse");
        let scratch = back.session.last().expect("a session");
        assert_eq!(scratch.name, "y");
        assert_eq!(scratch.window[0].pane[0].confidence, Confidence::Shell);
    }

    #[test]
    fn an_empty_snapshot_round_trips_too() {
        let back = parse(&render(&Snapshot::default())).expect("parse");
        assert_eq!(back, Snapshot::default());
        assert_eq!(back.attach_target(), None);
    }

    #[test]
    fn a_format_this_build_does_not_know_is_refused_by_number() {
        let mut snap = laptop();
        snap.header.format = 99;
        let text = toml::to_string_pretty(&snap).expect("render");
        let err = parse(&text).expect_err("should refuse");
        let said = err.to_string();
        assert!(said.contains("99"), "{said}");
        assert!(said.contains(&FORMAT.to_string()), "{said}");
    }

    #[test]
    fn a_key_the_reader_does_not_know_is_an_error_not_a_shrug() {
        let text = "[header]\nformat = 1\nwhat_is_this = true\n";
        assert!(parse(text).is_err());
    }

    #[test]
    fn a_newer_format_is_named_as_newer_even_when_it_carries_keys_this_build_lacks() {
        // The whole point of probing the number first: a format-2 file has
        // keys this build does not know, and refusing it as "unknown field"
        // would hide that the fix is to upgrade.
        let text = "[header]\nformat = 2\nsomething_new = true\n";
        let said = parse(text).expect_err("should refuse").to_string();
        assert!(said.contains("snapshot format 2"), "{said}");
        assert!(said.contains("newer tmux-companion"), "{said}");
        assert!(!said.contains("unknown field"), "{said}");
    }

    #[test]
    fn the_stamp_is_the_digits_after_the_plus_and_nothing_after_them() {
        assert_eq!(build_stamp("0.2.0+1790400979"), Some(1_790_400_979));
        assert_eq!(build_stamp("0.2.0+1790400979.abc1234"), Some(1_790_400_979));
        assert_eq!(
            build_stamp("0.2.0+1790400979.abc1234-dirty"),
            Some(1_790_400_979)
        );
        assert_eq!(build_stamp("0.2.0"), None);
        assert_eq!(build_stamp("0.2.0+"), None);
        assert_eq!(build_stamp("0.2.0+abc"), None);
    }

    #[test]
    fn newer_is_decided_by_number_and_never_by_string() {
        assert!(built_after("0.2.0+1790400979", "0.2.0+1790261531"));
        assert!(!built_after("0.2.0+1790261531", "0.2.0+1790400979"));
        assert!(!built_after("0.2.0+1790400979", "0.2.0+1790400979"));
        // `+999` would sort above `+1790400979` as text.
        assert!(!built_after("0.2.0+999", "0.2.0+1790400979"));
        // Nothing to compare is not evidence of anything.
        assert!(!built_after("0.2.0", "0.2.0+1790400979"));
        assert!(!built_after("0.2.0+1790400979", "0.2.0"));
    }

    #[test]
    fn a_snapshot_says_when_a_newer_build_wrote_it() {
        let mut snap = laptop();
        // This build's stamp, plus a day.
        let mine = build_stamp(&crate::proto::build_id()).expect("this build has a stamp");
        snap.header.companion_version = format!("0.9.0+{}", mine + 86_400);
        assert_eq!(
            snap.written_by_newer_build(),
            Some(snap.header.companion_version.as_str())
        );

        snap.header.companion_version = format!("0.1.0+{}", mine.saturating_sub(1));
        assert_eq!(snap.written_by_newer_build(), None);
        snap.header.companion_version = crate::proto::build_id();
        assert_eq!(snap.written_by_newer_build(), None);
        snap.header.companion_version = String::new();
        assert_eq!(snap.written_by_newer_build(), None);
    }

    #[test]
    fn the_attach_target_is_the_header_session_when_it_is_still_there() {
        assert_eq!(laptop().attach_target(), Some("tmux-companion"));
    }

    #[test]
    fn the_attach_target_falls_back_when_the_header_names_a_session_that_went() {
        let mut snap = laptop();
        snap.header.attached = "gone".to_string();
        assert_eq!(snap.attach_target(), Some("icf-c_com"));
    }
}
