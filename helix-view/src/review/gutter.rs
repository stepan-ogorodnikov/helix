//! The stream's base and source line numbers, its `+` and `-` signs, and its
//! diagnostics' dots. They take the place of the line number, diff, and
//! diagnostics gutters in the stream's views.

use std::fmt::Write;

use helix_core::diagnostic::Severity;

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

/// Upstream's dots, without its filter by the buffer's own servers. The
/// stream has none, and its diagnostics are its files'.
pub(crate) fn diagnostics<'doc>(
    editor: &'doc Editor,
    doc: &'doc Document,
    theme: &Theme,
) -> Option<GutterFn<'doc>> {
    editor.review.stream_of(doc.id())?;
    let warning = theme.get("warning");
    let error = theme.get("error");
    let info = theme.get("info");
    let hint = theme.get("hint");
    let diagnostics = doc.diagnostics();
    Some(Box::new(
        move |line: usize, _selected: bool, first_visual_line: bool, out: &mut String| {
            if !first_visual_line {
                return None;
            }
            let first = diagnostics.partition_point(|d| d.line < line);
            let worst = diagnostics[first..]
                .iter()
                .take_while(|d| d.line == line)
                .max_by_key(|d| d.severity)?;
            out.push('●');
            Some(match worst.severity {
                Some(Severity::Error) => error,
                Some(Severity::Warning) | None => warning,
                Some(Severity::Info) => info,
                Some(Severity::Hint) => hint,
            })
        },
    ))
}
