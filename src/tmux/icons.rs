// Nerd Font icon constants — codepoints confirmed from yrl's statusline_test.go
pub const ADDED: &str = "\u{f034c} ";
pub const AHEAD: &str = "\u{f40a} ";
pub const ARROW_RIGHT: &str = "\u{e0bc}";
// Lower-right triangle separator; used only by the net segment.
pub const ARROW_LEFT: &str = "\u{e0ba}";
// Slanted caps for the current-window block in the window segment.
// Same slant family as ARROW_LEFT so the left and right halves of the status
// bar share one shape language.
pub const SLANT_IN: &str = "\u{e0ba}";
pub const SLANT_OUT: &str = "\u{e0bc}";
// Bandwidth-rate units for the net segment.  Each one stands in for the whole
// unit ("KiB/s", "MiB/s", "GiB/s") to keep the segment narrow.  The commented
// alternatives are the md-alpha_k / md-alpha_m / md-alpha_g Nerd Font glyphs.
// Tests reference these by name, so swapping them needs no test edits — only
// that they stay distinct and never begin with a digit.
// pub const RATE_KIB: &str = "\u{f0af8}";
pub const RATE_KIB: &str = "K";
// pub const RATE_MIB: &str = "\u{f0afa}";
pub const RATE_MIB: &str = "M";
// pub const RATE_GIB: &str = "\u{f0af4}";
pub const RATE_GIB: &str = "G";
// Outline-style end caps.  ARROW_RIGHT is a solid triangle: it works as the
// boundary of a filled segment, but with no fill behind it it floats.  These
// are thin alternatives — preview them with `tmux-companion preview`.
pub const CAP_SLASH: &str = " \u{e0bd}"; // thin slash, same slant as ARROW_RIGHT
pub const CAP_CHEVRON: &str = " \u{e0b1}"; // thin right chevron
pub const CAP_RULE: &str = " \u{2502}"; // plain vertical rule
pub const CAP_EIGHTH: &str = " \u{258f}"; // left one-eighth block
pub const CAP_NONE: &str = ""; // nothing at all
pub const BEHIND: &str = "\u{f409} ";
// Fallback branch-type glyph (md-source_branch) for names with no recognized
// prefix — main, master, dev, stable, ...
pub const BRANCH: &str = "\u{f062c} ";
pub const BUGFIX: &str = "\u{f188} ";
pub const CHORE: &str = "\u{f19a1} ";
pub const CLEAN: &str = "\u{ebb1}";
pub const COPIED: &str = "\u{f0191} ";
pub const DELETED: &str = "\u{f0ad3} ";
pub const FAILED: &str = "\u{f04e7}";
pub const FEATURE: &str = "\u{f0eb} ";
pub const GIT: &str = "\u{f418} ";
pub const GONE: &str = "\u{f0d1} ";
pub const HOTFIX: &str = "\u{f06d} ";
pub const MODIFIED: &str = "\u{f1787} ";
pub const NEW: &str = "\u{f0a0} ";
pub const RELEASE: &str = "\u{f296} ";
pub const RENAMED: &str = "\u{ebcb} ";
// Marks the end of the git segment, keeping it off whatever tmux draws next.
// Inside the segment, groups are separated by plain spaces.
pub const SEPARATOR: &str = "\u{e621}";
pub const STAGED: &str = "\u{f01c} ";
pub const STASHED: &str = "\u{e257} ";
pub const SYNC: &str = "\u{f1378}";
pub const TAG: &str = "\u{f412} ";
pub const UNMERGED: &str = "\u{f1a98} ";
pub const WHITE_SPACE: &str = " ";
