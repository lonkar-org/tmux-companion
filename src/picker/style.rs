//! What a picker looks like, as settings rather than as a shape compiled into
//! the drawing.
//!
//! Every field here exists because somebody's tmux already looks a particular
//! way and the picker has to join it rather than argue with it. The defaults
//! are the tool's own look; the reason they are defaults and not constants is
//! that a person who has been driving fzf for years has a layout in their hands
//! and no interest in relearning it.

use serde::{Deserialize, Serialize};

/// Which line the border is drawn with, or none at all.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BorderKind {
    /// No border. The popup's own border, if tmux drew one, is all there is.
    None,
    /// Square corners.
    Plain,
    /// Round corners, which is what `--border=rounded` draws.
    #[default]
    Rounded,
    /// Double lines.
    Double,
    /// Heavy lines.
    Thick,
}

impl BorderKind {
    /// The ratatui border set, or `None` when nothing is drawn.
    pub fn set(self) -> Option<ratatui::symbols::border::Set<'static>> {
        use ratatui::symbols::border;
        match self {
            BorderKind::None => None,
            BorderKind::Plain => Some(border::PLAIN),
            BorderKind::Rounded => Some(border::ROUNDED),
            BorderKind::Double => Some(border::DOUBLE),
            BorderKind::Thick => Some(border::THICK),
        }
    }

    /// The horizontal line this border draws, used for the rules inside it so
    /// a rule and the border it meets are the same weight.
    pub fn horizontal(self) -> &'static str {
        self.set().map_or("\u{2500}", |s| s.horizontal_top)
    }
}

/// Where a label sits on the box it names.
///
/// Six positions rather than an offset pair, because every one of them is
/// somewhere a person would actually put a label and a free pair of
/// coordinates is a way to put one off the edge.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LabelPosition {
    /// Not drawn.
    Hidden,
    /// On the top line, `label_offset` cells in from the left corner.
    TopLeft,
    /// Centred on the top line.
    TopCenter,
    /// On the top line, `label_offset` cells in from the right corner.
    TopRight,
    /// On the bottom line, `label_offset` cells in from the left corner.
    BottomLeft,
    /// Centred on the bottom line.
    BottomCenter,
    /// On the bottom line, `label_offset` cells in from the right corner.
    /// This is fzf's `--border-label-pos=-3:bottom`.
    #[default]
    BottomRight,
}

impl LabelPosition {
    /// Whether this sits on the top edge. The bottom is the other one.
    pub fn on_top(self) -> bool {
        matches!(
            self,
            LabelPosition::TopLeft | LabelPosition::TopCenter | LabelPosition::TopRight
        )
    }

    /// Where the label starts, given how wide the box is and how long the
    /// label is.
    ///
    /// `offset` is how many cells of border are left showing beyond it, which
    /// is what `--border-label-pos=-3:bottom` means and what makes a label read
    /// as sitting on the line rather than as having replaced its corner.
    ///
    /// Pure, and the part worth testing: a label wider than the box it names
    /// has to land at zero rather than at a negative number nothing can draw.
    pub fn start(self, box_width: u16, label_width: u16, offset: u16) -> u16 {
        let room = box_width.saturating_sub(label_width);
        match self {
            LabelPosition::Hidden => 0,
            LabelPosition::TopLeft | LabelPosition::BottomLeft => offset.min(room),
            LabelPosition::TopCenter | LabelPosition::BottomCenter => room / 2,
            LabelPosition::TopRight | LabelPosition::BottomRight => room.saturating_sub(offset),
        }
    }
}

/// How much of a border the preview pane gets.
///
/// Three answers because fzf has three: `--preview-window=right,40%` draws a
/// box, `…,border-left` draws only the edge between the two panes, and
/// `…,border-none` draws nothing. Which one is right depends on what is in
/// there -- a directory listing wants a divider and a theme card wants a frame
/// around it -- so it is a setting and not a shape.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PreviewBorder {
    /// Nothing.
    None,
    /// Only the side facing the list, which is fzf's `border-left` and the
    /// rest of that family.
    #[default]
    Edge,
    /// All four sides.
    Full,
}

impl PreviewBorder {
    /// Which sides to draw, given where the preview sits.
    ///
    /// Pure, and worth its own test: the edge facing the list is a different
    /// side for each of the four positions, and getting one wrong draws a line
    /// down the outside of the popup.
    pub fn sides(self, preview: crate::picker::Preview) -> ratatui::widgets::Borders {
        use crate::picker::Preview;
        use ratatui::widgets::Borders;
        match self {
            PreviewBorder::None => Borders::NONE,
            PreviewBorder::Full => Borders::ALL,
            PreviewBorder::Edge => match preview {
                Preview::Right => Borders::LEFT,
                Preview::Left => Borders::RIGHT,
                Preview::Bottom => Borders::TOP,
                Preview::Top => Borders::BOTTOM,
                Preview::None => Borders::NONE,
            },
        }
    }
}

/// Which end of the list a line sits at.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Edge {
    /// Above the list.
    #[default]
    Top,
    /// Below the list.
    Bottom,
    /// Not drawn at all.
    Hidden,
}

/// The whole look of a picker, as one value.
///
/// Held by `Chrome` and filled from `[picker]`, so changing it changes every
/// picker at once rather than five call sites drifting apart. A picker that
/// wants its own answer overrides the one field it cares about.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Look {
    /// The line the outer box is drawn with.
    pub border: BorderKind,
    /// Where the picker's own label sits on that box.
    pub label_position: LabelPosition,
    /// How many cells of border are left showing beyond the label, the corner
    /// counted among them. Two is what fzf's `-3:bottom` draws.
    pub label_offset: u16,
    /// Which end the line naming the keys sits at.
    pub hint_position: Edge,
    /// Which end the query sits at. `bottom` is fzf's default layout and
    /// `top` is its `--reverse`.
    pub prompt_position: Edge,
    /// Which end the first row sits at.
    ///
    /// `bottom` is fzf's default: the best match is nearest the query with the
    /// list growing up away from it, so the cursor starts on the row your eye
    /// is already on. It is a separate setting from `prompt_position` because
    /// fzf couples the two and `--reverse-list` exists precisely because the
    /// coupling is wrong for some lists.
    pub list_from: Edge,
    /// Whether the `matched/total` counter is drawn. fzf's `--no-info`.
    pub counter: bool,
    /// Rules between the hint, the list and the query. Without them the three
    /// run together and the list has no edges of its own.
    pub rules: bool,
    /// Drawn in front of the row the cursor is on.
    pub marker: String,
    /// The order a row's columns are drawn in, by the position the picker
    /// built them at. Empty draws them as built.
    ///
    /// Whether the key or what it does comes first is the kind of thing people
    /// have an opinion about and no argument for, so it is a list rather than
    /// a decision. A position no row has is skipped, and a position left out
    /// is a column that is not drawn, which is how a picker is made narrower
    /// without touching the code that fills it.
    pub column_order: Vec<usize>,
    /// The fewest columns a list may be left with before a preview beside it
    /// is moved underneath instead.
    ///
    /// Zero turns the rule off, which is what fzf does: it splits whatever it
    /// is given and truncates the rows.
    pub min_list_width: u16,
    /// How much of a border the preview pane gets.
    pub preview_border: PreviewBorder,
    /// Where the preview's label sits on that line.
    pub preview_label_position: LabelPosition,
    /// How many cells of that line are left showing beyond the label.
    pub preview_label_offset: u16,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            border: BorderKind::Rounded,
            label_position: LabelPosition::BottomRight,
            label_offset: 2,
            hint_position: Edge::Top,
            prompt_position: Edge::Bottom,
            list_from: Edge::Top,
            counter: false,
            rules: true,
            marker: "\u{258c}".to_string(),
            column_order: Vec::new(),
            min_list_width: 24,
            preview_border: PreviewBorder::Edge,
            preview_label_position: LabelPosition::BottomCenter,
            preview_label_offset: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_right_hand_label_leaves_the_offset_showing_beyond_it() {
        // `[ Keys ]` on a 40-wide box with 3 cells of border after it. The
        // cells are what stop the label reading as a replaced corner; two of
        // them, the corner counted, is what fzf's `-3:bottom` draws.
        assert_eq!(LabelPosition::BottomRight.start(40, 8, 3), 29);
        assert_eq!(LabelPosition::BottomLeft.start(40, 8, 3), 3);
        assert_eq!(LabelPosition::BottomCenter.start(40, 8, 3), 16);
    }

    #[test]
    fn a_label_wider_than_its_box_starts_at_the_edge() {
        // Not a negative number, and not a panic: a narrow popup is a thing
        // that happens and the label is what gets clipped.
        assert_eq!(LabelPosition::BottomRight.start(6, 20, 3), 0);
        assert_eq!(LabelPosition::BottomLeft.start(6, 20, 3), 0);
        assert_eq!(LabelPosition::BottomCenter.start(6, 20, 3), 0);
    }

    #[test]
    fn the_top_positions_are_the_ones_on_the_top_edge() {
        assert!(LabelPosition::TopLeft.on_top());
        assert!(LabelPosition::TopCenter.on_top());
        assert!(LabelPosition::TopRight.on_top());
        assert!(!LabelPosition::BottomRight.on_top());
        assert!(!LabelPosition::Hidden.on_top());
    }

    #[test]
    fn the_previews_border_faces_the_list_whichever_side_it_is_on() {
        use crate::picker::Preview;
        use ratatui::widgets::Borders;
        // Getting one of these wrong draws a line down the outside of the
        // popup instead of between the two panes.
        assert_eq!(PreviewBorder::Edge.sides(Preview::Right), Borders::LEFT);
        assert_eq!(PreviewBorder::Edge.sides(Preview::Left), Borders::RIGHT);
        assert_eq!(PreviewBorder::Edge.sides(Preview::Bottom), Borders::TOP);
        assert_eq!(PreviewBorder::Edge.sides(Preview::Top), Borders::BOTTOM);

        assert_eq!(PreviewBorder::Full.sides(Preview::Right), Borders::ALL);
        assert_eq!(PreviewBorder::None.sides(Preview::Right), Borders::NONE);
    }

    #[test]
    fn a_rule_is_the_same_weight_as_the_border_it_meets() {
        assert_eq!(BorderKind::Thick.horizontal(), "\u{2501}");
        assert_eq!(BorderKind::Double.horizontal(), "\u{2550}");
        assert_eq!(BorderKind::Rounded.horizontal(), "\u{2500}");
        // No border still needs a rule to draw with.
        assert_eq!(BorderKind::None.horizontal(), "\u{2500}");
    }
}
