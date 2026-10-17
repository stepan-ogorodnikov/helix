//! The stream's base and source line numbers, and its `+` and `-` signs. They
//! take the place of the line number and diff gutters in the stream's views.

use std::fmt::Write;

use crate::gutter::GutterFn;
use crate::{Document, Editor, Theme};

use super::Row;

pub(crate) fn line_numbers<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    theme: &Theme,
    is_focused: bool,
) -> Option<GutterFn<'doc>> {
    let stream = editor.review.stream_of(doc.id())?;
    let linenr = theme.get("ui.linenr");
    let linenr_select = theme.get("ui.linenr.selected");
    let [base_width, source_width] = stream.widths;
    Some(Box::new(
        move |line: usize, selected: bool, first_visual_line: bool, out: &mut String| {
            let row = stream.rows.get(line).filter(|_| first_visual_line)?;
            let (base, source) = row.lines();
            let number =
                |line: Option<usize>| line.map_or(String::new(), |line| (line + 1).to_string());
            write!(
                out,
                "{:>base_width$} {:>source_width$}",
                number(base),
                number(source),
            )
            .unwrap();
            Some(if selected && is_focused {
                linenr_select
            } else {
                linenr
            })
        },
    ))
}

pub(crate) fn signs<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    theme: &Theme,
) -> Option<GutterFn<'doc>> {
    let stream = editor.review.stream_of(doc.id())?;
    let added = theme.get("diff.plus.gutter");
    let removed = theme.get("diff.minus.gutter");
    Some(Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            let (sign, style) = match stream.rows.get(line).filter(|_| first_visual_line)? {
                Row::Added { .. } => ('+', added),
                Row::Removed { .. } => ('-', removed),
                _ => return None,
            };
            out.push(sign);
            Some(style)
        },
    ))
}
