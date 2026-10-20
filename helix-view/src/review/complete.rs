//! What a language server's completion needs from the review, past what
//! `lend` gives it. A row holds its file's line char for char, so a place in
//! the file is the same character of the row that shows its line, in any
//! offset encoding. An item's edit moves onto the stream by its line numbers
//! alone. The edits it makes elsewhere, like an import, wait until the sync has
//! forwarded the item to the file.

use std::path::PathBuf;

use helix_lsp::{lsp, util::generate_transaction_from_edits, OffsetEncoding};

use super::{edit_file, sync::view_of, Review, Row, Stream};
use crate::{Document, Editor, ViewId};

/// An accepted item's other edits, for the file it was asked of.
pub(super) struct Deferred {
    path: PathBuf,
    edits: Vec<lsp::TextEdit>,
    encoding: OffsetEncoding,
}

/// The open buffer of the file under the focused view's primary cursor, when
/// the view shows the stream.
pub fn file_at_cursor(editor: &Editor) -> Option<&Document> {
    let view = editor.tree.get(editor.tree.focus);
    let stream = editor.review.stream_of(view.doc)?;
    let file = file_on(stream, cursor_row(&editor.documents[&view.doc], view.id))?;
    editor.document_by_path(stream.files[file].change.path())
}

fn cursor_row(doc: &Document, view: ViewId) -> usize {
    let text = doc.text().slice(..);
    text.char_to_line(doc.selection(view).primary().cursor(text))
}

fn file_on(stream: &Stream, row: usize) -> Option<usize> {
    match *stream.rows.get(row)? {
        Row::Context { file, .. } | Row::Added { file, .. } => Some(file),
        _ => None,
    }
}

/// `item`, with its edit on the stream's rows when `doc` is the stream.
pub fn item_on_rows(
    review: &Review,
    doc: &Document,
    view: ViewId,
    item: &lsp::CompletionItem,
) -> lsp::CompletionItem {
    let mut item = item.clone();
    if let Some(stream) = review.stream_of(doc.id()) {
        to_rows(stream, cursor_row(doc, view), &mut item);
    }
    item
}

/// Moves an item being accepted onto the stream's rows when `doc` is the
/// stream, and takes its other edits, to make in the file once the sync has
/// forwarded the item.
pub fn accept_item(
    review: &mut Review,
    doc: &Document,
    view: ViewId,
    item: &mut lsp::CompletionItem,
    encoding: OffsetEncoding,
) {
    let Some(stream) = review
        .stream
        .as_mut()
        .filter(|stream| stream.doc == doc.id())
    else {
        return;
    };
    let file = to_rows(stream, cursor_row(doc, view), item);
    // WHY: upstream applies these to the buffer the item goes in. In the
    // stream, they'd land on whatever rows have their line numbers.
    let edits = item.additional_text_edits.take().unwrap_or_default();
    stream.deferred = file.filter(|_| !edits.is_empty()).map(|file| Deferred {
        path: stream.files[file].change.path().to_path_buf(),
        edits,
        encoding,
    });
}

/// The file of `row`, after moving the item's edit onto the rows that show
/// its lines.
fn to_rows(stream: &Stream, row: usize, item: &mut lsp::CompletionItem) -> Option<usize> {
    let Some(file) = file_on(stream, row) else {
        // WHY: the edit would land on whatever row has its line number. At
        // the cursor, on a row the file doesn't have, the sync refuses it.
        item.text_edit = None;
        return None;
    };
    let Some(edit) = &mut item.text_edit else {
        return Some(file);
    };
    let row_of = |line: u32| {
        stream.rows.iter().position(|row| {
            matches!(*row, Row::Context { file: shown, source, .. } | Row::Added { file: shown, source }
                if shown == file && source == line as usize)
        })
    };
    // WHY: an edit over several lines needs them on rows one after another. A
    // removed row or a gap between them isn't the file's.
    let rows: Option<Vec<_>> = ranges(edit)
        .iter()
        .map(|range| {
            let (start, end) = (row_of(range.start.line)?, row_of(range.end.line)?);
            let lines = range.end.line.checked_sub(range.start.line)? as usize;
            (end.checked_sub(start)? == lines).then_some((start, end))
        })
        .collect();
    let Some(rows) = rows else {
        // WHY: the edit reaches a line the review doesn't show. The item goes
        // in at the cursor instead, over the word before it.
        item.text_edit = None;
        return Some(file);
    };
    for (range, (start, end)) in ranges(edit).into_iter().zip(rows) {
        range.start.line = start as u32;
        range.end.line = end as u32;
    }
    Some(file)
}

fn ranges(edit: &mut lsp::CompletionTextEdit) -> Vec<&mut lsp::Range> {
    match edit {
        lsp::CompletionTextEdit::Edit(edit) => vec![&mut edit.range],
        lsp::CompletionTextEdit::InsertAndReplace(edit) => {
            vec![&mut edit.insert, &mut edit.replace]
        }
    }
}

/// Makes the deferred edits in their file, after the sync forwarded the
/// item, unless the forward was `refused`.
pub(super) fn make_deferred(editor: &mut Editor, refused: bool) {
    let Some(stream) = editor.review.stream.as_mut() else {
        return;
    };
    let Some(Deferred {
        path,
        edits,
        encoding,
    }) = stream.deferred.take()
    else {
        return;
    };
    let stream = editor.review.stream.as_ref().expect("checked above");
    let seen = stream
        .files
        .iter()
        .find(|file| file.change.path() == path)
        .and_then(|file| file.seen);
    let stream_doc = stream.doc;
    let Some(doc) = editor.document_by_path(&path).filter(|_| !refused) else {
        return;
    };
    // WHY: the edits are places in the text the server answered from, with
    // the item in it. A buffer that moved since has other places.
    if seen != Some((doc.id(), doc.version())) {
        let message = format!(
            "{} changed, so the completion's other edits weren't made",
            doc.display_name()
        );
        editor.set_error(message);
        return;
    }
    let file = doc.id();
    let transaction = generate_transaction_from_edits(doc.text(), edits, encoding);
    let Some(view) = view_of(editor, stream_doc) else {
        return;
    };
    edit_file(editor, file, view, &transaction);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use helix_core::Rope;
    use helix_vcs::FileChange;

    use super::*;
    use crate::review::tests::stream_of;
    use crate::review::{Content, File, Section};

    fn file(name: &str, base: &str, source: &str) -> File {
        File::from(Section {
            change: FileChange::Modified {
                path: PathBuf::from(name),
            },
            content: Content::Text {
                base: Rope::from(base),
                source: Rope::from(source),
            },
            encoding: helix_core::encoding::UTF_8,
        })
    }

    fn range(start: u32, end: u32) -> lsp::Range {
        lsp::Range::new(lsp::Position::new(start, 1), lsp::Position::new(end, 0))
    }

    fn replace(start: u32, end: u32) -> lsp::CompletionTextEdit {
        lsp::CompletionTextEdit::Edit(lsp::TextEdit::new(range(start, end), "x".into()))
    }

    fn both(insert: lsp::Range, replace: lsp::Range) -> lsp::CompletionTextEdit {
        lsp::CompletionTextEdit::InsertAndReplace(lsp::InsertReplaceEdit {
            new_text: "x".into(),
            insert,
            replace,
        })
    }

    /// Two files as the stream shows them:
    ///
    /// ```text
    /// 0   a.txt  modified
    /// 1   a           context, a.txt line 0
    /// 2   b           context, a.txt line 1
    /// 3   c           removed
    /// 4   C           added, a.txt line 2
    /// 5   d           context, a.txt line 3
    /// 6   e           context, a.txt line 4
    /// 7   f           context, a.txt line 5
    /// 8   ⋯ 5 unchanged lines, a.txt lines 6-10
    /// 9   b.txt  modified
    /// 10  x           removed
    /// 11  y           added, b.txt line 0
    /// ```
    #[test]
    fn an_items_edit_moves_to_the_rows_that_show_its_lines() {
        let base = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let stream = stream_of(vec![
            file("a.txt", base, &base.replace('c', "C")),
            file("b.txt", "x\n", "y\n"),
        ]);
        let cases = [
            (
                4,
                replace(2, 2),
                Some(0),
                Some(replace(4, 4)),
                "on the cursor's row",
            ),
            (
                5,
                replace(3, 4),
                Some(0),
                Some(replace(5, 6)),
                "over two rows",
            ),
            (
                4,
                both(range(2, 2), range(2, 3)),
                Some(0),
                Some(both(range(4, 4), range(4, 5))),
                "both ranges",
            ),
            (
                11,
                replace(0, 0),
                Some(1),
                Some(replace(11, 11)),
                "in the cursor's file",
            ),
            (2, replace(1, 2), Some(0), None, "over a removed row"),
            (4, replace(8, 8), Some(0), None, "on a line in a gap"),
            (3, replace(2, 2), None, None, "from a removed row"),
            (0, replace(2, 2), None, None, "from a header"),
        ];
        for (row, edit, file, moved, case) in cases {
            let mut item = lsp::CompletionItem {
                text_edit: Some(edit),
                ..Default::default()
            };
            assert_eq!(to_rows(&stream, row, &mut item), file, "{case}");
            assert_eq!(item.text_edit, moved, "{case}");
        }
    }
}
