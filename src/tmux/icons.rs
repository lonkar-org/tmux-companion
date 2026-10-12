//! Nerd Font codepoints the status bar draws with.
//!
//! Codepoints were confirmed against yrl's `statusline_test.go`. Tests refer to
//! these by name rather than by literal, so changing a codepoint here
//! propagates to every expectation without a test edit; see the `ARROW_RIGHT`
//! invariant in `CLAUDE.md`.
//!
//! Everything here assumes a patched font. The glyph presets in the port plan
//! are what give a reader without one something legible instead.

/// Added file, staged (md-file_plus).
pub const ADDED: &str = "\u{f034c} ";
/// A coding agent, drawn before the count in the `agents` segment (md-robot).
pub const AGENT: &str = "\u{f06a9} ";
/// nf-oct-arrow_right, between the agent count and the waiting count.
pub const AGENT_TO: &str = "\u{f432} ";
/// nf-fa-hourglass_start, the agents segment's "waiting" in its glyph style.
pub const WAITING: &str = "\u{f251} ";

/// nf-fa-question_circle: an agent stopped on a question for you.
pub const ASKED: &str = "\u{f059} ";

/// nf-cod-terminal_tmux: a pane somebody is reading in copy mode.
pub const READING: &str = "\u{ebc8} ";
/// `nf-md-progress_check`: the busy count on the `agents` segment.
pub const BUSY: &str = "\u{f0996} ";
/// nf-md-bell_alert_outline, the mark the health segment draws: something
/// wants your attention.
pub const HEALTH: &str = "\u{f0e81} ";
/// Commits this branch has that its upstream does not (md-arrow_up).
pub const AHEAD: &str = "\u{f40a} ";
/// Solid right-pointing triangle, the powerline wedge between segments.
pub const ARROW_RIGHT: &str = "\u{e0bc}";
/// Lower-right triangle separator; used only by the net segment.
pub const ARROW_LEFT: &str = "\u{e0ba}";
/// Opening slanted cap of the current-window block in the window segment.
///
/// Same slant family as [`ARROW_LEFT`] so the left and right halves of the
/// status bar share one shape language.
pub const SLANT_IN: &str = "\u{e0ba}";
/// Closing slanted cap of the current-window block.
pub const SLANT_OUT: &str = "\u{e0bc}";
// Bandwidth-rate units for the net segment.  Each one stands in for the whole
// unit ("KiB/s", "MiB/s", "GiB/s") to keep the segment narrow.  The commented
// alternatives are the md-alpha_k / md-alpha_m / md-alpha_g Nerd Font glyphs.
// Tests reference these by name, so swapping them needs no test edits — only
// that they stay distinct and never begin with a digit.
// pub const RATE_KIB: &str = "\u{f0af8}";
/// Kibibytes per second, standing in for the whole unit.
pub const RATE_KIB: &str = "K";
// pub const RATE_MIB: &str = "\u{f0afa}";
/// Mebibytes per second.
pub const RATE_MIB: &str = "M";
// pub const RATE_GIB: &str = "\u{f0af4}";
/// Gibibytes per second.
pub const RATE_GIB: &str = "G";
// Outline-style end caps.  ARROW_RIGHT is a solid triangle: it works as the
// boundary of a filled segment, but with no fill behind it it floats.  These
// are thin alternatives — preview them with `tmux-companion preview`.
/// Thin slash end cap, same slant as [`ARROW_RIGHT`].
pub const CAP_SLASH: &str = " \u{e0bd}";
/// Thin right chevron end cap.
pub const CAP_CHEVRON: &str = " \u{e0b1}";
/// Plain vertical rule end cap.
pub const CAP_RULE: &str = " \u{2502}";
/// Left one-eighth block end cap.
pub const CAP_EIGHTH: &str = " \u{258f}";
/// No end cap at all.
pub const CAP_NONE: &str = "";
/// Commits the upstream has that this branch does not (md-arrow_down).
pub const BEHIND: &str = "\u{f409} ";
/// Fallback branch-type glyph (md-source_branch) for names with no recognised
/// prefix: main, master, dev, stable and the rest.
pub const BRANCH: &str = "\u{f062c} ";
/// Branch named like a bug fix.
pub const BUGFIX: &str = "\u{f188} ";
/// Branch named like a chore.
pub const CHORE: &str = "\u{f19a1} ";
/// Nothing to commit, nothing unpushed.
pub const CLEAN: &str = "\u{ebb1}";
/// Copied file, staged.
pub const COPIED: &str = "\u{f0191} ";
/// Deleted file.
pub const DELETED: &str = "\u{f0ad3} ";
/// The last remote operation failed.
pub const FAILED: &str = "\u{f04e7}";
/// Branch named like a feature.
pub const FEATURE: &str = "\u{f0eb} ";
/// Git itself, drawn before the branch name when `--branch-icon` is set.
pub const GIT: &str = "\u{f418} ";
/// The upstream branch is gone.
pub const GONE: &str = "\u{f0d1} ";
/// Branch named like a hotfix.
pub const HOTFIX: &str = "\u{f06d} ";
/// Modified file.
pub const MODIFIED: &str = "\u{f1787} ";
/// Untracked file.
pub const NEW: &str = "\u{f0a0} ";
/// Branch named like a release.
pub const RELEASE: &str = "\u{f296} ";
/// Renamed file.
pub const RENAMED: &str = "\u{ebcb} ";
/// Marks the end of the git segment, keeping it off whatever tmux draws next.
///
/// Inside the segment, groups are separated by plain spaces.
pub const SEPARATOR: &str = "\u{e621}";
/// Staged file.
pub const STAGED: &str = "\u{f01c} ";
/// At least one stash entry exists.
pub const STASHED: &str = "\u{e257} ";
/// A fetch or push is in flight.
pub const SYNC: &str = "\u{f1378}";
/// The checkout is on a tag rather than a branch.
pub const TAG: &str = "\u{f412} ";
// ── Branch types ────────────────────────────────────────────────────────────
//
// The glyph in front of a branch name, by how the name starts. The first six
// are above, where they always were; these are the rest of the built-in table
// in `config::default_branch_types`, all Material Design (md), which is one
// family on one grid and a plane nothing else on the bar draws from.

/// Branch named like documentation (md-file_document).
pub const DOCS: &str = "\u{f0219} ";
/// Branch named like performance work (md-speedometer).
pub const PERF: &str = "\u{f04c5} ";
/// Branch named like a refactor (md-recycle).
pub const REFACTOR: &str = "\u{f044c} ";
/// Branch named like tests (md-test_tube).
pub const TEST: &str = "\u{f0668} ";
/// Branch named like CI work (md-cog_sync).
pub const CI: &str = "\u{f1460} ";
/// Branch named like build work (md-hammer).
pub const BUILD: &str = "\u{f08ea} ";
/// Branch named like a dependency bump (md-package_variant).
pub const DEPS: &str = "\u{f03d6} ";
/// Branch named like style or formatting (md-format_paint).
pub const STYLE: &str = "\u{f027c} ";
/// Branch named like a revert (md-undo).
pub const REVERT: &str = "\u{f054c} ";
/// Branch named like work in progress (md-progress_wrench).
pub const WIP: &str = "\u{f0cbd} ";
/// Branch named like an experiment or a spike (md-flask).
pub const EXPERIMENT: &str = "\u{f0093} ";
/// Branch named like benchmarks (md-chart_line).
pub const BENCH: &str = "\u{f012a} ";
/// Branch parked for later (md-parking).
pub const PARKED: &str = "\u{f03e3} ";
/// Branch named like security work (md-shield_lock).
pub const SECURITY: &str = "\u{f099d} ";
/// Branch named like accessibility work (md-human_handsup).
pub const A11Y: &str = "\u{f064c} ";
/// Branch named like translation work (md-translate).
pub const I18N: &str = "\u{f05ca} ";
/// Branch named like design work (md-pencil_ruler).
pub const DESIGN: &str = "\u{f1353} ";
/// Branch named like research (md-microscope).
pub const RESEARCH: &str = "\u{f0654} ";
/// Branch named like a backup (md-archive).
pub const BACKUP: &str = "\u{f003c} ";
/// Any other `word/` prefix that neither the table nor the glyph-name lookup
/// places (md-label_outline). The name is drawn whole after it.
pub const PREFIXED: &str = "\u{f0316} ";

/// File with merge conflicts.
pub const UNMERGED: &str = "\u{f1a98} ";
/// A single space, named so a format string reads as what it draws.
pub const WHITE_SPACE: &str = " ";

// ── Popups ──────────────────────────────────────────────────────────────────
//
// What a row in a picker is, drawn in the column in front of it. No trailing
// space, unlike the bar's: the picker pads its icon column itself.

/// A tmux session (fa-window_restore): the project picker's live rows.
pub const SESSION: &str = "\u{f2d2}";

/// Another machine (nf-md-ssh): a host's group line in the session
/// switcher, and the mark after `@vm` on a mirror session's bar.
pub const SSH: &str = "\u{f08c0}";

/// A directory (fa-folder): a project not yet open, a place for a window.
pub const FOLDER: &str = "\u{f07b}";

/// A project directory (nf-md-puzzle_edit), in the list that holds the
/// sessions too: a different shape from [`SESSION`], where the folder was
/// close enough to read as one.
pub const PROJECT: &str = "\u{f14d3}";

/// A path nobody has opened yet (fa-plus).
pub const PATH_NEW: &str = "\u{f067}";

/// A pane running a shell or a program (fa-terminal).
pub const PANE: &str = "\u{f120}";

/// A listening port (fa-plug).
pub const PORT: &str = "\u{f1e6}";

/// Searching scrollback (fa-search).
pub const SEARCH: &str = "\u{f002}";

/// A key binding (fa-keyboard_o).
pub const KEY: &str = "\u{f11c}";

/// A command from history (fa-history).
pub const HISTORY: &str = "\u{f1da}";

/// Something setup can add (fa-wrench).
pub const SETUP: &str = "\u{f0ad}";

/// The journal (fa-book).
pub const JOURNAL: &str = "\u{f02d}";

/// A colour theme (fa-paint_brush).
pub const THEME: &str = "\u{f1fc}";

/// A snapshot of the server (fa-camera).
pub const SNAPSHOT: &str = "\u{f030}";

/// Quiet hours (md-bell_off_outline): the bells are off on purpose.
pub const QUIET: &str = "\u{f0a91}";

/// On, done, fine (fa-check).
pub const CHECK: &str = "\u{f00c}";

/// Not done yet (fa-circle_o).
pub const OPEN_ITEM: &str = "\u{f10c}";

/// Skipped on purpose (fa-ban).
pub const SKIPPED: &str = "\u{f05e}";

/// Cannot tell (fa-question).
pub const UNKNOWN: &str = "\u{f128}";

/// The chunk clock under its budget (md-timer_check), before the time.
pub const TIMER_CHECK: &str = "\u{f1ad0} ";

/// The chunk clock past its budget (md-timer_alert), before the time.
pub const TIMER_ALERT: &str = "\u{f1acc} ";

/// A time, or something that ran long (fa-clock_o).
pub const CLOCK: &str = "\u{f017}";

/// Run it (fa-play).
pub const RUN: &str = "\u{f04b}";

/// Nobody has looked in days (fa-bed).
pub const IDLE: &str = "\u{f236}";

/// Closed (fa-times).
pub const CLOSED: &str = "\u{f00d}";

/// Battery, empty (fa-battery-empty).
pub const BATTERY_EMPTY: &str = "\u{f244}";
/// Battery, a quarter (fa-battery-quarter).
pub const BATTERY_QUARTER: &str = "\u{f243}";
/// Battery, half (fa-battery-half).
pub const BATTERY_HALF: &str = "\u{f242}";
/// Battery, three quarters (fa-battery-three-quarters).
pub const BATTERY_THREE_QUARTERS: &str = "\u{f241}";
/// Battery, full (fa-battery-full).
pub const BATTERY_FULL: &str = "\u{f240}";
/// On mains power (fa-plug).
pub const PLUGGED: &str = "\u{f1e6}";

/// Download, in front of its rate when the bar has no colour to say it
/// (fa-arrow-down).
pub const RATE_DOWN: &str = "\u{f063} ";
/// Upload, the same (fa-arrow-up).
pub const RATE_UP: &str = "\u{f062} ";

/// Every glyph by the name of its constant, in one list, so `by_name` and
/// anything that needs all of them (the branch lookup steering clear of the
/// bar's own glyphs) cannot disagree.
pub const ALL: &[(&str, &str)] = &[
    ("ADDED", ADDED),
    ("AGENT", AGENT),
    ("HEALTH", HEALTH),
    ("WAITING", WAITING),
    ("BUSY", BUSY),
    ("AGENT_TO", AGENT_TO),
    ("AHEAD", AHEAD),
    ("ARROW_RIGHT", ARROW_RIGHT),
    ("ARROW_LEFT", ARROW_LEFT),
    ("SLANT_IN", SLANT_IN),
    ("SLANT_OUT", SLANT_OUT),
    ("RATE_KIB", RATE_KIB),
    ("RATE_MIB", RATE_MIB),
    ("RATE_GIB", RATE_GIB),
    ("CAP_SLASH", CAP_SLASH),
    ("CAP_CHEVRON", CAP_CHEVRON),
    ("CAP_RULE", CAP_RULE),
    ("CAP_EIGHTH", CAP_EIGHTH),
    ("CAP_NONE", CAP_NONE),
    ("BEHIND", BEHIND),
    ("BRANCH", BRANCH),
    ("BUGFIX", BUGFIX),
    ("CHORE", CHORE),
    ("CLEAN", CLEAN),
    ("COPIED", COPIED),
    ("DELETED", DELETED),
    ("FAILED", FAILED),
    ("FEATURE", FEATURE),
    ("GIT", GIT),
    ("GONE", GONE),
    ("HOTFIX", HOTFIX),
    ("MODIFIED", MODIFIED),
    ("NEW", NEW),
    ("RELEASE", RELEASE),
    ("RENAMED", RENAMED),
    ("SEPARATOR", SEPARATOR),
    ("STAGED", STAGED),
    ("STASHED", STASHED),
    ("SYNC", SYNC),
    ("TAG", TAG),
    ("UNMERGED", UNMERGED),
    ("WHITE_SPACE", WHITE_SPACE),
    ("SESSION", SESSION),
    ("SSH", SSH),
    ("FOLDER", FOLDER),
    ("PROJECT", PROJECT),
    ("PATH_NEW", PATH_NEW),
    ("PANE", PANE),
    ("PORT", PORT),
    ("SEARCH", SEARCH),
    ("KEY", KEY),
    ("HISTORY", HISTORY),
    ("SETUP", SETUP),
    ("JOURNAL", JOURNAL),
    ("THEME", THEME),
    ("SNAPSHOT", SNAPSHOT),
    ("QUIET", QUIET),
    ("CHECK", CHECK),
    ("OPEN_ITEM", OPEN_ITEM),
    ("SKIPPED", SKIPPED),
    ("UNKNOWN", UNKNOWN),
    ("CLOCK", CLOCK),
    ("ASKED", ASKED),
    ("READING", READING),
    ("TIMER_CHECK", TIMER_CHECK),
    ("TIMER_ALERT", TIMER_ALERT),
    ("RUN", RUN),
    ("IDLE", IDLE),
    ("CLOSED", CLOSED),
    ("BATTERY_EMPTY", BATTERY_EMPTY),
    ("BATTERY_QUARTER", BATTERY_QUARTER),
    ("BATTERY_HALF", BATTERY_HALF),
    ("BATTERY_THREE_QUARTERS", BATTERY_THREE_QUARTERS),
    ("BATTERY_FULL", BATTERY_FULL),
    ("PLUGGED", PLUGGED),
    ("RATE_DOWN", RATE_DOWN),
    ("RATE_UP", RATE_UP),
    ("DOCS", DOCS),
    ("PERF", PERF),
    ("REFACTOR", REFACTOR),
    ("TEST", TEST),
    ("CI", CI),
    ("BUILD", BUILD),
    ("DEPS", DEPS),
    ("STYLE", STYLE),
    ("REVERT", REVERT),
    ("WIP", WIP),
    ("EXPERIMENT", EXPERIMENT),
    ("BENCH", BENCH),
    ("PARKED", PARKED),
    ("SECURITY", SECURITY),
    ("A11Y", A11Y),
    ("I18N", I18N),
    ("DESIGN", DESIGN),
    ("RESEARCH", RESEARCH),
    ("BACKUP", BACKUP),
    ("PREFIXED", PREFIXED),
];

/// Look a glyph up by the name of its constant.
///
/// This is what lets a config file say `STAGED = "*"`: the name in the file is
/// the name in this module, so there is no second list of glyph names to keep
/// in step with this one.
pub fn by_name(name: &str) -> Option<&'static str> {
    ALL.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, glyph)| *glyph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_constant_is_reachable_by_name() {
        // The config refers to glyphs by these names, so a constant that
        // `by_name` does not know is a glyph nobody can override.
        for (name, expected) in [
            ("STAGED", STAGED),
            ("ARROW_RIGHT", ARROW_RIGHT),
            ("CAP_NONE", CAP_NONE),
            ("WHITE_SPACE", WHITE_SPACE),
            ("SEPARATOR", SEPARATOR),
        ] {
            assert_eq!(by_name(name), Some(expected), "{name}");
        }
    }

    #[test]
    fn an_unknown_name_is_none_rather_than_a_default() {
        assert_eq!(by_name("STAGD"), None);
        assert_eq!(by_name(""), None);
    }
}
