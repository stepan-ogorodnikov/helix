//! How the review stream looks: tints behind removed and added rows, styled
//! header and gap rows with a rule after each header, and changed words.
//!
//! Theme scopes, each with a fallback so bundled themes still tell rows apart:
//!
//! - `diff.minus.view` and `diff.plus.view` tint removed and added rows, from
//!   #15631. Removed text falls back to `diff.minus`.
//! - `diff.delta.view.text` marks changed words. It has no fallback.
//! - `ui.review.header` falls back to `markup.heading`, and `ui.review.gap` to
//!   `comment`. The rule after a header uses `ui.window`.
//!
//! Syntax colors come first, from parses of each file's two sides, so these
//! styles patch over them.

use helix_core::syntax::{Highlight, OverlayHighlights};
use helix_core::unicode::width::UnicodeWidthStr;
use helix_view::graphics::Rect;
use helix_view::review::{self, Row};
use helix_view::{Document, Editor, Theme, View};

use crate::ui::document::{LinePos, TextRenderer};
use crate::ui::text_decorations::DecorationManager;

pub fn render<'a>(
    editor: &'a Editor,
    doc: &'a Document,
    view: &View,
    theme: &Theme,
    decorations: &mut DecorationManager<'a>,
    overlays: &mut Vec<OverlayHighlights>,
) {
    let Some(stream) = editor.review.stream_of(doc.id()) else {
        return;
    };
    // WHY: syntax colors on removed rows show only when `diff.minus.view` sets
    // no text color. Most themes fall back to `diff.minus`, a text color.
    let base = theme
        .try_get("diff.minus.view")
        .is_none_or(|style| style.fg.is_none());
    let text = doc.text();
    let first = text.char_to_line(doc.view_offset(view.id).anchor.min(text.len_chars()));
    let height = view.inner_area(doc).height as usize;
    overlays.extend(review::syntax_overlays(
        editor,
        stream,
        first..first + height,
        base,
    ));

    let ranges = &stream.ranges;
    let either = |scope: &str, fallback: &str| {
        theme
            .find_highlight_exact(scope)
            .or_else(|| theme.find_highlight(fallback))
    };
    let mut overlay = |highlight: Option<Highlight>, ranges: &Vec<std::ops::Range<usize>>| {
        if let Some(highlight) = highlight.filter(|_| !ranges.is_empty()) {
            overlays.push(OverlayHighlights::Homogeneous {
                highlight,
                ranges: ranges.clone(),
            });
        }
    };
    overlay(theme.find_highlight("diff.minus.view"), &ranges.removed);
    overlay(
        either("ui.review.header", "markup.heading"),
        &ranges.headers,
    );
    overlay(either("ui.review.gap", "comment"), &ranges.gaps);
    overlay(
        theme.find_highlight_exact("diff.delta.view.text"),
        &ranges.words,
    );

    // WHY: exact lookups. `diff.plus` and `diff.minus` are text colors in most
    // themes, and a tint is a background.
    let added = theme.try_get_exact("diff.plus.view");
    let removed = theme.try_get_exact("diff.minus.view");
    let rule = theme.get("ui.window");
    decorations.add_decoration(move |renderer: &mut TextRenderer, pos: LinePos| {
        let viewport = renderer.viewport;
        let row = Rect::new(viewport.x, pos.visual_line, viewport.width, 1);
        match stream.rows.get(pos.doc_line) {
            Some(Row::Added { .. }) => {
                if let Some(style) = added {
                    renderer.set_style(row, style);
                }
            }
            Some(Row::Removed { .. }) => {
                if let Some(style) = removed {
                    renderer.set_style(row, style);
                }
            }
            Some(Row::Header { .. }) if pos.first_visual_line => {
                let header = text.line(pos.doc_line).to_string();
                let width = header.trim_end().width() + 1;
                let start = width.saturating_sub(renderer.offset.col);
                let Some(rest) = (viewport.width as usize).checked_sub(start) else {
                    return;
                };
                let line = "─".repeat(rest);
                renderer.set_stringn(
                    viewport.x + start as u16,
                    pos.visual_line,
                    &line,
                    rest,
                    rule,
                );
            }
            _ => {}
        }
    });
}
