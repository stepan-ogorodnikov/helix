//! FORK: inline diff. Removed base lines show as virtual rows above the lines
//! that replaced them. This module reserves the rows. The painting lives in
//! `helix-term/src/ui/text_decorations/diff.rs`.

use std::ops::Range;

use helix_core::text_annotations::{LineAnnotation, TextAnnotations};
use helix_core::{Position, Rope, RopeSlice};
use helix_vcs::Hunk;

use crate::{Document, View};

/// A copy of a document's diff.
///
/// The diff handle hands out a read lock. Rendering takes it again for the
/// gutter, and the fair lock can deadlock if the diff worker queues for its
/// write lock in between. So copy what's needed and release the lock.
#[derive(Debug, Clone)]
pub struct DiffSnapshot {
    pub base: Rope,
    pub hunks: Vec<Hunk>,
}

impl DiffSnapshot {
    pub fn load(doc: &Document) -> Option<Self> {
        let diff = doc.diff_handle()?.load();
        let hunks = (0..diff.len()).map(|i| diff.nth_hunk(i)).collect();
        Some(DiffSnapshot {
            base: diff.diff_base().clone(),
            hunks,
        })
    }

    /// The removed base lines of each hunk, in document order.
    ///
    /// The diff can lag the buffer by a frame while you type, so hunk lines
    /// are clamped to `text`.
    pub fn removed_rows(&self, text: RopeSlice) -> Vec<RemovedRows> {
        let base_len = self.base.len_lines();
        self.hunks
            .iter()
            .filter_map(|hunk| {
                let base_lines = (hunk.before.start as usize).min(base_len)
                    ..(hunk.before.end as usize).min(base_len);
                if base_lines.is_empty() {
                    return None;
                }
                // Rows can only follow a line ending, so a hunk at line 0 shows
                // below line 0. The last line has no line ending.
                let displaced = hunk.after.start == 0;
                let line = (hunk.after.start as usize).max(1);
                if line >= text.len_lines() {
                    return None;
                }
                Some(RemovedRows {
                    anchor: text.line_to_char(line),
                    base_lines,
                    displaced,
                })
            })
            .collect()
    }
}

/// Removed base lines shown above one document line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedRows {
    /// The char index at the start of the line the rows sit above.
    /// `LineAnnotation::insert_virtual_lines` gets it at the end of the line before.
    pub anchor: usize,
    /// Base lines, one row each.
    pub base_lines: Range<usize>,
    /// The hunk starts at line 0, so the rows sit below line 0 instead.
    pub displaced: bool,
}

/// Adds the removed rows of `doc`'s diff when `view` has the inline diff on.
pub(crate) fn add_line_annotation(view: &View, doc: &Document, annotations: &mut TextAnnotations) {
    if !view.inline_diff {
        return;
    }
    if let Some(diff) = DiffSnapshot::load(doc) {
        let rows = diff.removed_rows(doc.text().slice(..));
        annotations.add_line_annotation(Box::new(InlineDiff { rows, next: 0 }));
    }
}

struct InlineDiff {
    rows: Vec<RemovedRows>,
    next: usize,
}

impl LineAnnotation for InlineDiff {
    fn reset_pos(&mut self, char_idx: usize) -> usize {
        // Rows anchored at `char_idx` belong to the line before the traversal.
        self.next = self.rows.partition_point(|rows| rows.anchor <= char_idx);
        usize::MAX
    }

    fn insert_virtual_lines(
        &mut self,
        line_end_char_idx: usize,
        _line_end_visual_pos: Position,
        _doc_line: usize,
    ) -> Position {
        // Soft-wrapped line ends land mid-line, before any anchor.
        let mut height = 0;
        while let Some(rows) = self.rows.get(self.next) {
            if rows.anchor > line_end_char_idx {
                break;
            }
            if rows.anchor == line_end_char_idx {
                height += rows.base_lines.len();
            }
            self.next += 1;
        }
        Position::new(height, 0)
    }
}

#[cfg(test)]
mod tests {
    use helix_core::doc_formatter::TextFormat;
    use helix_core::visual_offset_from_block;

    use super::*;

    fn hunk(before: Range<u32>, after: Range<u32>) -> Hunk {
        Hunk { before, after }
    }

    fn snapshot(base: &str, hunks: Vec<Hunk>) -> DiffSnapshot {
        DiffSnapshot {
            base: Rope::from(base),
            hunks,
        }
    }

    /// The visual row of each line of `text`, with `diff`'s removed rows reserved.
    fn line_rows(text: &str, diff: Option<&DiffSnapshot>, text_fmt: &TextFormat) -> Vec<usize> {
        let text = Rope::from(text);
        let text = text.slice(..);
        let mut annotations = TextAnnotations::default();
        if let Some(diff) = diff {
            annotations.add_line_annotation(Box::new(InlineDiff {
                rows: diff.removed_rows(text),
                next: 0,
            }));
        }
        (0..text.len_lines())
            .map(|line| {
                let pos = text.line_to_char(line);
                visual_offset_from_block(text, 0, pos, text_fmt, &annotations)
                    .0
                    .row
            })
            .collect()
    }

    #[test]
    fn modified_lines_get_rows_above() {
        let diff = snapshot("a\nold1\nold2\nc\n", vec![hunk(1..3, 1..2)]);
        let rows = diff.removed_rows(Rope::from("a\nnew\nc\n").slice(..));
        assert_eq!(
            rows,
            vec![RemovedRows {
                anchor: 2,
                base_lines: 1..3,
                displaced: false,
            }]
        );
        assert_eq!(
            line_rows("a\nnew\nc\n", Some(&diff), &TextFormat::default()),
            [0, 3, 4, 5]
        );
    }

    #[test]
    fn pure_removal_sits_above_the_next_line() {
        let diff = snapshot("a\ngone\nb\n", vec![hunk(1..2, 1..1)]);
        assert_eq!(
            line_rows("a\nb\n", Some(&diff), &TextFormat::default()),
            [0, 2, 3]
        );
    }

    #[test]
    fn insertions_reserve_nothing() {
        let diff = snapshot("a\nb\n", vec![hunk(1..1, 1..2)]);
        assert!(diff
            .removed_rows(Rope::from("a\nnew\nb\n").slice(..))
            .is_empty());
    }

    #[test]
    fn line_zero_hunk_shows_below_line_zero() {
        let diff = snapshot("old\nb\n", vec![hunk(0..1, 0..1)]);
        let rows = diff.removed_rows(Rope::from("new\nb\n").slice(..));
        assert_eq!(rows[0].anchor, 4);
        assert!(rows[0].displaced);
        assert_eq!(
            line_rows("new\nb\n", Some(&diff), &TextFormat::default()),
            [0, 2, 3]
        );
    }

    #[test]
    fn hunks_sharing_an_anchor_stack() {
        // A removal at line 0, then a change at line 1. Both show after line 0.
        let diff = snapshot(
            "gone\nkeep\nold\nz\n",
            vec![hunk(0..1, 0..0), hunk(2..3, 1..2)],
        );
        assert_eq!(
            line_rows("keep\nnew\nz\n", Some(&diff), &TextFormat::default()),
            [0, 3, 4, 5]
        );
    }

    #[test]
    fn soft_wrap_does_not_trigger_rows_mid_line() {
        let text_fmt = TextFormat {
            soft_wrap: true,
            viewport_width: 6,
            ..TextFormat::default()
        };
        let text = "aaa bbb ccc\nnew\nz\n";
        let diff = snapshot("aaa bbb ccc\nold\nz\n", vec![hunk(1..2, 1..2)]);
        let plain = line_rows(text, None, &text_fmt);
        assert!(plain[1] > 1, "line 0 should wrap: {plain:?}");
        let with_diff = line_rows(text, Some(&diff), &text_fmt);
        // One removed row after line 0's last visual line, none at its wraps.
        let shift: Vec<_> = with_diff.iter().zip(&plain).map(|(a, b)| a - b).collect();
        assert_eq!(shift, [0, 1, 1, 1]);
    }

    #[test]
    fn rows_clamp_to_a_lagging_diff() {
        // The diff still sees four lines, but the buffer has two.
        let diff = snapshot("a\nb\nc\nd\n", vec![hunk(3..4, 3..3), hunk(9..12, 1..1)]);
        let rows = diff.removed_rows(Rope::from("a\nb\n").slice(..));
        assert!(rows.is_empty());
    }

    #[test]
    fn last_line_without_line_ending_has_no_rows_after_it() {
        let diff = snapshot("a\nb", vec![hunk(0..2, 0..1)]);
        assert!(diff.removed_rows(Rope::from("a").slice(..)).is_empty());
    }
}
