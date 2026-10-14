//! FORK: inline diff. Paints removed base lines into the rows that
//! `helix_view::annotations::diff` reserves, and tints changed lines.

use std::borrow::Cow;
use std::ops::Range;

use helix_core::doc_formatter::FormattedGrapheme;
use helix_core::graphemes::{grapheme_width, tab_width_at};
use helix_core::line_ending::{line_end_char_index, line_without_line_ending};
use helix_core::{Position, RopeSlice};
use helix_stdx::rope::RopeSliceExt;
use helix_view::annotations::diff::{DiffSnapshot, RemovedRows};
use helix_view::editor::GutterType;
use helix_view::graphics::Rect;
use helix_view::theme::Style;
use helix_view::{Document, Theme, View};

use crate::ui::document::{LinePos, TextRenderer};
use crate::ui::text_decorations::Decoration;

#[derive(Default)]
struct Styles {
    removed: Style,
    added_line: Option<Style>,
    modified_line: Option<Style>,
    gutter: Style,
    line_number: Style,
    removed_marker: Style,
}

impl Styles {
    fn new(theme: &Theme) -> Styles {
        Styles {
            // Falls back to `diff.minus`, a text color, which suits removed text.
            removed: theme.get("diff.minus.view"),
            // Exact lookups. A fallback lands on `diff.plus` or `diff.delta`,
            // text colors that would paint over syntax colors.
            added_line: theme.try_get_exact("diff.plus.view"),
            modified_line: theme.try_get_exact("diff.delta.view"),
            gutter: theme.get("ui.gutter.virtual"),
            line_number: theme.get("ui.linenr"),
            removed_marker: theme.get("diff.minus.gutter"),
        }
    }
}

/// A gutter column on screen: its kind, x, and width.
type GutterColumn = (GutterType, u16, usize);

/// Does nothing unless the view has the inline diff on and the document has a diff.
///
/// This reads its own copy of the diff, separate from the annotation's. If the
/// diff lands between the two, one frame is off, and the redraw the diff
/// worker requests fixes it.
pub struct InlineDiff<'a> {
    text: RopeSlice<'a>,
    diff: DiffSnapshot,
    rows: Vec<RemovedRows>,
    next_rows: usize,
    /// Rows whose preceding line ending was just rendered.
    pending: Range<usize>,
    next_hunk: usize,
    gutter: Vec<GutterColumn>,
    tab_width: u16,
    styles: Styles,
}

impl<'a> InlineDiff<'a> {
    pub fn new(doc: &'a Document, view: &View, theme: &Theme) -> Self {
        let text = doc.text().slice(..);
        let diff = view.inline_diff.then(|| DiffSnapshot::load(doc)).flatten();
        let Some(diff) = diff else {
            return InlineDiff {
                text,
                diff: DiffSnapshot {
                    base: Default::default(),
                    hunks: Vec::new(),
                },
                rows: Vec::new(),
                next_rows: 0,
                pending: 0..0,
                next_hunk: 0,
                gutter: Vec::new(),
                tab_width: 0,
                styles: Styles::default(),
            };
        };

        // `render_view` skips the gutter when the view is too narrow for it.
        let mut gutter = Vec::new();
        if view.gutter_offset(doc) != 0 {
            let mut x = view.area.x;
            for &kind in view.gutters() {
                let width = kind.width(view, doc);
                gutter.push((kind, x, width));
                x += width as u16;
            }
        }

        InlineDiff {
            text,
            rows: diff.removed_rows(text),
            diff,
            next_rows: 0,
            pending: 0..0,
            next_hunk: 0,
            gutter,
            tab_width: doc.tab_width() as u16,
            styles: Styles::new(theme),
        }
    }

    fn seek_rows(&mut self, char_idx: usize) -> usize {
        self.next_rows = self.rows.partition_point(|rows| rows.anchor <= char_idx);
        self.next_hook()
    }

    /// The line ending before the next rows. The rows render once it has.
    fn next_hook(&self) -> usize {
        let Some(rows) = self.rows.get(self.next_rows) else {
            return usize::MAX;
        };
        let line = self.text.char_to_line(rows.anchor);
        line_end_char_index(&self.text, line - 1)
    }

    fn draw_row(&self, renderer: &mut TextRenderer, row: usize, base_line: usize, displaced: bool) {
        let top = renderer.offset.row;
        if row < top || row >= top + renderer.viewport.height as usize {
            return;
        }
        let y = row as u16;
        let viewport = renderer.viewport;
        let style = renderer.text_style.patch(self.styles.removed);
        renderer.set_style(Rect::new(viewport.x, y, viewport.width, 1), style);

        let base = self.diff.base.slice(..);
        let line = line_without_line_ending(&base, base_line);
        let text = visible_text(
            line,
            self.tab_width,
            renderer.offset.col,
            viewport.width as usize,
        );
        renderer.set_stringn(viewport.x, y, &text, viewport.width as usize, style);

        for &(kind, x, width) in &self.gutter {
            renderer.set_style(Rect::new(x, y, width as u16, 1), self.styles.gutter);
            let (text, style) = match kind {
                // The gutter is sized for the buffer. Leave a base line number
                // that doesn't fit blank instead of cutting its digits.
                GutterType::LineNumbers => match format!("{:>width$}", base_line + 1) {
                    number if number.len() > width => continue,
                    number => (number, self.styles.line_number),
                },
                GutterType::Diff => (
                    if displaced { "↑" } else { "-" }.to_string(),
                    self.styles.removed_marker,
                ),
                _ => continue,
            };
            renderer.set_stringn(x, y, &text, width, self.styles.gutter.patch(style));
        }
    }
}

impl Decoration for InlineDiff<'_> {
    fn decorate_line(&mut self, renderer: &mut TextRenderer, pos: LinePos) {
        let hunks = &self.diff.hunks;
        while hunks
            .get(self.next_hunk)
            .is_some_and(|hunk| hunk.after.end as usize <= pos.doc_line)
        {
            self.next_hunk += 1;
        }
        let Some(hunk) = hunks.get(self.next_hunk) else {
            return;
        };
        if hunk.after.start as usize > pos.doc_line {
            return;
        }
        let style = if hunk.is_pure_insertion() {
            self.styles.added_line
        } else {
            self.styles.modified_line
        };
        if let Some(style) = style {
            let viewport = renderer.viewport;
            renderer.set_style(
                Rect::new(viewport.x, pos.visual_line, viewport.width, 1),
                style,
            );
        }
    }

    fn render_virt_lines(
        &mut self,
        renderer: &mut TextRenderer,
        pos: LinePos,
        virt_off: Position,
    ) -> Position {
        let first_row = pos.visual_line as usize + virt_off.row;
        let mut row = first_row;
        for rows in &self.rows[self.pending.clone()] {
            for base_line in rows.base_lines.clone() {
                self.draw_row(renderer, row, base_line, rows.displaced);
                row += 1;
            }
        }
        self.pending = 0..0;
        Position::new(row - first_row, 0)
    }

    fn reset_pos(&mut self, pos: usize) -> usize {
        let line = self.text.char_to_line(pos.min(self.text.len_chars()));
        self.next_hunk = self
            .diff
            .hunks
            .partition_point(|hunk| hunk.after.end as usize <= line);
        self.pending = 0..0;
        self.seek_rows(pos)
    }

    fn skip_concealed_anchor(&mut self, conceal_end_char_idx: usize) -> usize {
        self.seek_rows(conceal_end_char_idx)
    }

    fn decorate_grapheme(
        &mut self,
        _renderer: &mut TextRenderer,
        _grapheme: &FormattedGrapheme,
    ) -> usize {
        // Several hunks can share an anchor. They render together.
        let anchor = self.rows[self.next_rows].anchor;
        let end = self.rows[self.next_rows..]
            .iter()
            .position(|rows| rows.anchor != anchor)
            .map_or(self.rows.len(), |i| self.next_rows + i);
        self.pending = self.next_rows..end;
        self.next_rows = end;
        self.next_hook()
    }
}

/// The part of `line` from column `skip` that fits in `width` columns, with
/// tabs expanded. A grapheme cut by either edge becomes spaces.
fn visible_text(line: RopeSlice, tab_width: u16, skip: usize, width: usize) -> String {
    let end = skip + width;
    let mut out = String::new();
    let mut col = 0;
    for grapheme in line.graphemes() {
        if col >= end {
            break;
        }
        let grapheme = Cow::from(grapheme);
        let is_tab = grapheme == "\t";
        let next = col
            + if is_tab {
                tab_width_at(col, tab_width)
            } else {
                grapheme_width(&grapheme)
            };
        if col >= skip && next <= end && !is_tab {
            out.push_str(&grapheme);
        } else {
            let visible = next.min(end).saturating_sub(col.max(skip));
            out.extend(std::iter::repeat_n(' ', visible));
        }
        col = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use helix_core::Rope;

    use super::visible_text;

    fn visible(line: &str, skip: usize, width: usize) -> String {
        visible_text(Rope::from(line).slice(..), 4, skip, width)
    }

    #[test]
    fn expands_tabs_to_the_next_stop() {
        assert_eq!(visible("a\tb", 0, 10), "a   b");
        assert_eq!(visible("\t\tx", 0, 10), "        x");
    }

    #[test]
    fn scrolls_and_truncates() {
        assert_eq!(visible("abcdef", 2, 3), "cde");
        assert_eq!(visible("abc", 5, 3), "");
    }

    #[test]
    fn cut_graphemes_become_spaces() {
        // A tab scrolled half off the left edge.
        assert_eq!(visible("\tab", 2, 4), "  ab");
        // A wide grapheme cut by the right edge.
        assert_eq!(visible("a日b", 0, 2), "a ");
        // A wide grapheme cut by the left edge.
        assert_eq!(visible("日b", 1, 2), " b");
    }
}
