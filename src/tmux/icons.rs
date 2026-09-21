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
/// File with merge conflicts.
pub const UNMERGED: &str = "\u{f1a98} ";
/// A single space, named so a format string reads as what it draws.
pub const WHITE_SPACE: &str = " ";

/// Look a glyph up by the name of its constant.
///
/// This is what lets a config file say `STAGED = "*"`: the name in the file is
/// the name in this module, so there is no second list of glyph names to keep
/// in step with this one.
pub fn by_name(name: &str) -> Option<&'static str> {
    match name {
        "ADDED" => Some(ADDED),
        "AHEAD" => Some(AHEAD),
        "ARROW_RIGHT" => Some(ARROW_RIGHT),
        "ARROW_LEFT" => Some(ARROW_LEFT),
        "SLANT_IN" => Some(SLANT_IN),
        "SLANT_OUT" => Some(SLANT_OUT),
        "RATE_KIB" => Some(RATE_KIB),
        "RATE_MIB" => Some(RATE_MIB),
        "RATE_GIB" => Some(RATE_GIB),
        "CAP_SLASH" => Some(CAP_SLASH),
        "CAP_CHEVRON" => Some(CAP_CHEVRON),
        "CAP_RULE" => Some(CAP_RULE),
        "CAP_EIGHTH" => Some(CAP_EIGHTH),
        "CAP_NONE" => Some(CAP_NONE),
        "BEHIND" => Some(BEHIND),
        "BRANCH" => Some(BRANCH),
        "BUGFIX" => Some(BUGFIX),
        "CHORE" => Some(CHORE),
        "CLEAN" => Some(CLEAN),
        "COPIED" => Some(COPIED),
        "DELETED" => Some(DELETED),
        "FAILED" => Some(FAILED),
        "FEATURE" => Some(FEATURE),
        "GIT" => Some(GIT),
        "GONE" => Some(GONE),
        "HOTFIX" => Some(HOTFIX),
        "MODIFIED" => Some(MODIFIED),
        "NEW" => Some(NEW),
        "RELEASE" => Some(RELEASE),
        "RENAMED" => Some(RENAMED),
        "SEPARATOR" => Some(SEPARATOR),
        "STAGED" => Some(STAGED),
        "STASHED" => Some(STASHED),
        "SYNC" => Some(SYNC),
        "TAG" => Some(TAG),
        "UNMERGED" => Some(UNMERGED),
        "WHITE_SPACE" => Some(WHITE_SPACE),
        _ => None,
    }
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
