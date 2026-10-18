//! Puts each file's diagnostics on the rows that show their lines. An open
//! file's come from its buffer, which moves them as you type. A file that
//! isn't open gets what its servers reported for it, which the editor keeps by
//! path. A report dispatches no event for a file that isn't open, so the sync
//! compares the reports with the ones last placed instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use helix_core::diagnostic::{Diagnostic, DiagnosticProvider, Range};
use helix_core::line_ending::line_end_char_index;
use helix_core::Uri;
use helix_lsp::{lsp, OffsetEncoding};
use helix_vcs::ChangeView;

use super::{Content, Row};
use crate::{Document, Editor};

/// Each file's report, as the stream last placed it.
pub(super) type Reports = HashMap<PathBuf, Vec<(lsp::Diagnostic, DiagnosticProvider)>>;

fn report<'a>(editor: &'a Editor, path: &Path) -> &'a [(lsp::Diagnostic, DiagnosticProvider)] {
    editor
        .diagnostics
        .get(&Uri::from(path))
        .map_or(&[], Vec::as_slice)
}

/// Places the diagnostics again when a file's report changed since.
pub(super) fn follow(editor: &mut Editor) {
    let Some(stream) = editor.review.stream.as_ref() else {
        return;
    };
    let moved = stream.files.iter().any(|file| {
        let path = file.change.path();
        let placed = stream.reports.get(path).map_or(&[][..], Vec::as_slice);
        report(editor, path) != placed
    });
    if moved {
        place(editor);
    }
}

/// Puts each file's diagnostics on the rows that show their lines, and drops
/// the rest.
pub(super) fn place(editor: &mut Editor) {
    let Some(stream) = editor.review.stream.as_ref() else {
        return;
    };
    let Some(doc) = editor.documents.get(&stream.doc) else {
        return;
    };
    let text = doc.text().slice(..);
    let rows: HashMap<(usize, usize), usize> = stream
        .rows
        .iter()
        .enumerate()
        .filter_map(|(row, kind)| match *kind {
            Row::Context { file, source, .. } | Row::Added { file, source } => {
                Some(((file, source), row))
            }
            _ => None,
        })
        .collect();
    let loader = editor.syn_loader.load();

    let mut placed = Vec::new();
    let mut reports = Reports::new();
    for (index, file) in stream.files.iter().enumerate() {
        let path = file.change.path();
        let report = report(editor, path);
        if !report.is_empty() {
            reports.insert(path.to_path_buf(), report.to_vec());
        }
        let Content::Text { source, .. } = &file.content else {
            continue;
        };
        // WHY: a staged review's rows show the index, and the servers report
        // on the file.
        if stream.scope == ChangeView::Staged {
            continue;
        }
        let diagnostics: Vec<Diagnostic> = match editor.document_by_path(path) {
            // INVARIANT: the rows came from this version of the buffer, so its
            // positions are the source's. One that moved since waits for the
            // relayout, which places them again.
            Some(doc) if file.seen == Some((doc.id(), doc.version())) => doc.diagnostics().to_vec(),
            Some(_) => continue,
            None => {
                let config = file
                    .language
                    .map(|language| loader.language(language).config());
                report
                    .iter()
                    .filter_map(|(diagnostic, provider)| {
                        // WHY: a report outlives the server that sent it, and
                        // the protocol's default encoding still reads it.
                        let encoding = provider
                            .language_server_id()
                            .and_then(|id| editor.language_server_by_id(id))
                            .map_or(OffsetEncoding::Utf16, |server| server.offset_encoding());
                        Document::lsp_diagnostic_to_diagnostic(
                            source,
                            config.map(|config| &**config),
                            diagnostic,
                            provider.clone(),
                            encoding,
                        )
                    })
                    .collect()
            }
        };

        let at = |pos: usize| {
            let line = source.char_to_line(pos.min(source.len_chars()));
            let row = *rows.get(&(index, line))?;
            Some((
                row,
                text.line_to_char(row) + pos - source.line_to_char(line),
            ))
        };
        for diagnostic in diagnostics {
            let Some((row, start)) = at(diagnostic.range.start) else {
                continue;
            };
            // WHY: a range that ends on a line the review doesn't show ends
            // with its first row.
            let end = match at(diagnostic.range.end) {
                Some((end_row, end)) if end_row >= row => end,
                _ => line_end_char_index(&text, row),
            };
            placed.push(Diagnostic {
                range: Range {
                    start,
                    end: end.max(start),
                },
                line: row,
                ..diagnostic
            });
        }
    }
    drop(loader);

    let stream = editor.review.stream.as_mut().expect("checked above");
    stream.reports = reports;
    let doc = editor
        .documents
        .get_mut(&stream.doc)
        .expect("checked above");
    doc.replace_diagnostics(placed, &[], None);
}
