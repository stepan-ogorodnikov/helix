//! FORK: inline diff. Paints removed base lines into the rows that
//! `helix_view::annotations::diff` reserves, tints changed lines, and marks
//! the words that changed.

mod words;

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use helix_core::doc_formatter::{DocumentFormatter, FormattedGrapheme, TextFormat};
use helix_core::graphemes::Grapheme;
use helix_core::line_ending::{line_end_char_index, line_without_line_ending};
use helix_core::syntax::{Highlight, HighlightEvent, Loader, OverlayHighlights};
use helix_core::text_annotations::TextAnnotations;
use helix_core::{Position, RopeSlice, Syntax};
use helix_stdx::rope::RopeSliceExt;
use helix_view::annotations::diff::{row_height, DiffSnapshot, RemovedRows};
use helix_view::editor::GutterType;
use helix_view::graphics::Rect;
use helix_view::theme::Style;
use helix_view::{Document, Editor, Theme, View};

use crate::ui::document::{LinePos, TextRenderer};
use crate::ui::text_decorations::Decoration;

use words::{changed_words, ChangedWords};

struct Styles {
    removed: Style,
    added_line: Option<Style>,
    modified_line: Option<Style>,
    changed_words: Option<(Highlight, Style)>,
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
            changed_words: theme
                .find_highlight_exact("diff.delta.view.text")
                .map(|highlight| (highlight, theme.highlight(highlight))),
            gutter: theme.get("ui.gutter.virtual"),
            line_number: theme.get("ui.linenr"),
            removed_marker: theme.get("diff.minus.gutter"),
        }
    }
}

/// A gutter column on screen: its kind, x, and width.
type GutterColumn = (GutterType, u16, usize);

/// Syntax styles of base text, as sorted char ranges into the base.
type Spans = Vec<(Range<usize>, Style)>;

/// Does nothing unless the view has the inline diff on and the document has a diff.
///
/// This reads its own copy of the diff, separate from the annotation's. If the
/// diff lands between the two, one frame is off, and the redraw the diff
/// worker requests fixes it.
pub struct InlineDiff<'a>(Option<Painter<'a>>);

impl<'a> InlineDiff<'a> {
    pub fn new(editor: &'a Editor, doc: &'a Document, view: &View) -> Self {
        let diff = view
            .inline_diff
            .enabled
            .then(|| DiffSnapshot::load(doc))
            .flatten();
        InlineDiff(diff.map(|diff| Painter::new(editor, doc, view, diff)))
    }

    /// The changed words in view on the document side. Push it before the
    /// selection overlays so a selection shows over it.
    pub fn word_highlights(&self) -> Option<OverlayHighlights> {
        let painter = self.0.as_ref()?;
        let (highlight, _) = painter.styles.changed_words?;
        let ranges: Vec<_> = painter
            .words
            .iter()
            .flat_map(|(_, words)| words.doc.iter().cloned())
            .collect();
        (!ranges.is_empty()).then_some(OverlayHighlights::Homogeneous { highlight, ranges })
    }
}

impl Decoration for InlineDiff<'_> {
    fn decorate_line(&mut self, renderer: &mut TextRenderer, pos: LinePos) {
        if let Some(painter) = &mut self.0 {
            painter.decorate_line(renderer, pos);
        }
    }

    fn render_virt_lines(
        &mut self,
        renderer: &mut TextRenderer,
        pos: LinePos,
        virt_off: Position,
    ) -> Position {
        match &mut self.0 {
            Some(painter) => painter.render_virt_lines(renderer, pos, virt_off),
            None => Position::new(0, 0),
        }
    }

    fn reset_pos(&mut self, pos: usize) -> usize {
        self.0
            .as_mut()
            .map_or(usize::MAX, |painter| painter.reset_pos(pos))
    }

    fn skip_concealed_anchor(&mut self, conceal_end_char_idx: usize) -> usize {
        self.0.as_mut().map_or(usize::MAX, |painter| {
            painter.seek_rows(conceal_end_char_idx)
        })
    }

    fn decorate_grapheme(
        &mut self,
        _renderer: &mut TextRenderer,
        _grapheme: &FormattedGrapheme,
    ) -> usize {
        self.0
            .as_mut()
            .map_or(usize::MAX, |painter| painter.take_rows())
    }
}

struct Painter<'a> {
    text: RopeSlice<'a>,
    theme: &'a Theme,
    diff: DiffSnapshot,
    /// The base's syntax tree, once parsed, and the loader to query it.
    syntax: Option<(Arc<Syntax>, Arc<Loader>)>,
    text_fmt: TextFormat,
    rows: Vec<RemovedRows>,
    next_rows: usize,
    /// Rows whose preceding line ending was just rendered.
    pending: Range<usize>,
    next_hunk: usize,
    /// Changed words of the modified hunks that can be in view, by hunk index.
    words: Vec<(usize, ChangedWords)>,
    gutter: Vec<GutterColumn>,
    styles: Styles,
}

impl<'a> Painter<'a> {
    fn new(editor: &'a Editor, doc: &'a Document, view: &View, diff: DiffSnapshot) -> Self {
        let theme = &editor.theme;
        let text = doc.text().slice(..);
        let styles = Styles::new(theme);

        // Syntax colors show only when the removed style leaves the text color alone.
        let syntax = if styles.removed.fg.is_none() {
            doc.syntax().and_then(|syntax| {
                let loader = editor.syn_loader.load_full();
                let base_syntax =
                    view.inline_diff
                        .base_syntax(&diff.base, syntax.root_language(), &loader)?;
                Some((base_syntax, loader))
            })
        } else {
            None
        };

        let words = if styles.changed_words.is_some() {
            let first_line =
                text.char_to_line(doc.view_offset(view.id).anchor.min(text.len_chars()));
            words_in_view(
                &diff,
                text,
                first_line,
                view.inner_area(doc).height as usize,
            )
        } else {
            Vec::new()
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

        Painter {
            text,
            theme,
            rows: diff.removed_rows(text),
            diff,
            syntax,
            text_fmt: doc.text_format(view.inner_width(doc), Some(theme)),
            next_rows: 0,
            pending: 0..0,
            next_hunk: 0,
            words,
            gutter,
            styles,
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

    /// Marks the rows at the hook as pending and returns the next hook.
    fn take_rows(&mut self) -> usize {
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

    fn reset_pos(&mut self, pos: usize) -> usize {
        let line = self.text.char_to_line(pos.min(self.text.len_chars()));
        self.next_hunk = self
            .diff
            .hunks
            .partition_point(|hunk| hunk.after.end as usize <= line);
        self.pending = 0..0;
        self.seek_rows(pos)
    }

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
        let pending = std::mem::replace(&mut self.pending, 0..0);
        for rows in &self.rows[pending] {
            let spans = match &self.syntax {
                Some((syntax, loader)) => syntax_spans(
                    syntax,
                    loader,
                    self.theme,
                    self.diff.base.slice(..),
                    rows.base_lines.clone(),
                ),
                None => Vec::new(),
            };
            let words = self.base_words(rows.hunk);
            for base_line in rows.base_lines.clone() {
                row += self.draw_line(renderer, row, base_line, rows.displaced, &spans, words);
            }
        }
        Position::new(row - first_row, 0)
    }

    /// The changed words of hunk `hunk` on the base side.
    fn base_words(&self, hunk: usize) -> &[Range<usize>] {
        match self.words.binary_search_by_key(&hunk, |(i, _)| *i) {
            Ok(i) => &self.words[i].1.base,
            Err(_) => &[],
        }
    }

    /// Draws a removed base line from `row` and returns the rows it took.
    fn draw_line(
        &self,
        renderer: &mut TextRenderer,
        row: usize,
        base_line: usize,
        displaced: bool,
        spans: &[(Range<usize>, Style)],
        words: &[Range<usize>],
    ) -> usize {
        let base = self.diff.base.slice(..);
        // The annotation reserved this many rows.
        let height = row_height(base, base_line, &self.text_fmt);
        let top = renderer.offset.row;
        let bottom = top + renderer.viewport.height as usize;
        let visible = |row: usize| row >= top && row < bottom;
        if !(row..row + height).any(visible) {
            return height;
        }

        let viewport = renderer.viewport;
        let row_style = renderer.text_style.patch(self.styles.removed);
        for i in (0..height).filter(|&i| visible(row + i)) {
            let y = (row + i) as u16;
            renderer.set_style(Rect::new(viewport.x, y, viewport.width, 1), row_style);
            self.draw_gutter(renderer, y, (i == 0).then_some(base_line), displaced);
        }

        let line = line_without_line_ending(&base, base_line);
        let line_start = base.line_to_char(base_line);
        let annotations = TextAnnotations::default();
        let formatter =
            DocumentFormatter::new_at_prev_checkpoint(line, &self.text_fmt, &annotations, 0);
        let (mut span, mut word) = (0, 0);
        for grapheme in formatter {
            let y = row + grapheme.visual_pos.row;
            if grapheme.source.is_eof() || !visible(y) {
                continue;
            }
            let style = if grapheme.is_virtual() {
                // The soft-wrap indicator.
                let highlight = self.text_fmt.wrap_indicator_highlight;
                let indicator = highlight.map(|highlight| self.theme.highlight(highlight));
                row_style.patch(indicator.unwrap_or_default())
            } else {
                let char_idx = line_start + grapheme.char_idx;
                let mut style = renderer.text_style;
                if let Some((_, syntax)) = find(spans, &mut span, char_idx, |(range, _)| range) {
                    style = style.patch(*syntax);
                }
                style = style.patch(self.styles.removed);
                if find(words, &mut word, char_idx, |range| range).is_some() {
                    let changed = self.styles.changed_words.map(|(_, style)| style);
                    style = style.patch(changed.unwrap_or_default());
                }
                style
            };
            draw_grapheme(renderer, y as u16, &grapheme, style);
        }
        height
    }

    /// Fills the gutter of a removed row. `base_line` is set on its first row.
    fn draw_gutter(
        &self,
        renderer: &mut TextRenderer,
        y: u16,
        base_line: Option<usize>,
        displaced: bool,
    ) {
        for &(kind, x, width) in &self.gutter {
            renderer.set_style(Rect::new(x, y, width as u16, 1), self.styles.gutter);
            let Some(base_line) = base_line else {
                continue;
            };
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

/// Word diffs of the modified hunks that can show in a view of `height` rows
/// from `first_line`.
fn words_in_view(
    diff: &DiffSnapshot,
    text: RopeSlice,
    first_line: usize,
    height: usize,
) -> Vec<(usize, ChangedWords)> {
    // Every line takes at least one row.
    let last_line = first_line + height;
    let lines = |range: &Range<u32>| range.start as usize..range.end as usize;
    let base = diff.base.slice(..);
    let start = diff
        .hunks
        .partition_point(|hunk| (hunk.after.end as usize) < first_line);
    diff.hunks[start..]
        .iter()
        .take_while(|hunk| hunk.after.start as usize <= last_line)
        .enumerate()
        .filter(|(_, hunk)| !hunk.before.is_empty() && !hunk.after.is_empty())
        .filter_map(|(i, hunk)| {
            let words = changed_words(base, lines(&hunk.before), text, lines(&hunk.after))?;
            Some((start + i, words))
        })
        .collect()
}

/// The syntax styles of `lines` in `base`.
fn syntax_spans(
    syntax: &Syntax,
    loader: &Loader,
    theme: &Theme,
    base: RopeSlice,
    lines: Range<usize>,
) -> Spans {
    let to_char = |byte: u32| base.byte_to_char(base.ceil_char_boundary(byte as usize));
    let start = base.line_to_byte(lines.start) as u32;
    let end = base.line_to_byte(lines.end) as u32;

    let mut highlighter = syntax.highlighter(base, loader, start..end);
    let mut spans = Vec::new();
    let mut style = Style::default();
    let mut pos = start;
    loop {
        let next = highlighter.next_event_offset().min(end);
        if next > pos && style != Style::default() {
            spans.push((to_char(pos)..to_char(next), style));
        }
        if next >= end {
            return spans;
        }
        pos = pos.max(next);
        let (event, highlights) = highlighter.advance();
        let base_style = match event {
            HighlightEvent::Refresh => Style::default(),
            HighlightEvent::Push => style,
        };
        style = highlights.fold(base_style, |acc, highlight| {
            acc.patch(theme.highlight(highlight))
        });
    }
}

/// Finds the item of sorted, disjoint `items` that contains `char_idx`.
/// `next` remembers the place for the next call with a larger `char_idx`.
fn find<'i, T>(
    items: &'i [T],
    next: &mut usize,
    char_idx: usize,
    range: impl Fn(&T) -> &Range<usize>,
) -> Option<&'i T> {
    while items
        .get(*next)
        .is_some_and(|item| range(item).end <= char_idx)
    {
        *next += 1;
    }
    items
        .get(*next)
        .filter(|item| range(item).start <= char_idx)
}

/// Draws a grapheme of a removed row. A grapheme cut by the view's edge stays blank.
fn draw_grapheme(renderer: &mut TextRenderer, y: u16, grapheme: &FormattedGrapheme, style: Style) {
    let (col, width) = (grapheme.visual_pos.col, grapheme.width());
    if !renderer.column_in_bounds(col, width) {
        return;
    }
    let symbol = match &grapheme.raw {
        Grapheme::Tab { width } => Cow::Owned(" ".repeat(*width)),
        Grapheme::Other { g } if &**g == "\u{00A0}" => Cow::Borrowed(" "),
        Grapheme::Other { g } => Cow::Borrowed(&**g),
        Grapheme::Newline => Cow::Borrowed(" "),
    };
    let x = renderer.viewport.x + (col - renderer.offset.col) as u16;
    renderer.set_string(x, y, &symbol, style);
}

#[cfg(test)]
mod tests {
    use helix_core::Rope;
    use helix_view::graphics::Color;

    use super::*;

    #[test]
    fn syntax_spans_cover_only_the_lines() {
        let theme: toml::Value = toml::from_str("keyword = \"red\"\nstring = \"green\"").unwrap();
        let theme = Theme::from(theme);
        let loader = helix_core::config::default_lang_loader();
        loader.set_scopes(theme.scopes().to_vec());
        let language = loader.language_for_name("rust").unwrap();
        let base = Rope::from("let a = 1;\nlet s = \"x\";\n");
        let syntax = Syntax::new(base.slice(..), language, &loader).unwrap();

        let spans = syntax_spans(&syntax, &loader, &theme, base.slice(..), 1..2);
        let red = Style::default().fg(Color::Red);
        let green = Style::default().fg(Color::Green);
        // Line 1 starts at char 11.
        assert_eq!(spans, [(11..14, red), (19..22, green)]);
    }

    #[test]
    fn find_walks_sorted_ranges() {
        let ranges = [2..4, 6..7];
        let mut next = 0;
        let found: Vec<_> = (0..8)
            .map(|i| find(&ranges, &mut next, i, |range| range).is_some())
            .collect();
        assert_eq!(found, [false, false, true, true, false, false, true, false]);
    }
}
