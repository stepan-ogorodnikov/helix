//! Readies the file of the row under the cursor for an LSP command. The stream
//! has no path and no language servers, so `helix-term` runs upstream's
//! command with the view lent to the file's buffer, which this module loads
//! and puts the view's selection in, at the same place as in the stream.

use helix_core::line_ending::line_end_char_index;
use helix_core::{Range, RopeSlice, Selection};
use helix_vcs::ChangeView;

use super::{load, Content, Row, Stream};
use crate::{DocumentId, Editor};

/// The file, source line, and column of a stream position on an added or
/// unchanged line.
fn place(stream: &Stream, text: RopeSlice, pos: usize) -> Option<(usize, usize, usize)> {
    let row = text.char_to_line(pos);
    let (Row::Context { file, source, .. } | Row::Added { file, source }) =
        *stream.rows.get(row)?
    else {
        return None;
    };
    Some((file, source, pos - text.line_to_char(row)))
}

/// The buffer of the file under the focused view's primary cursor, loaded if
/// need be, with the view's selection at the same place in it. Or why there's
/// none to use.
pub fn lend(editor: &mut Editor) -> Result<DocumentId, String> {
    let view = editor.tree.focus;
    let stream_doc = editor.tree.get(view).doc;
    let Some(stream) = editor.review.stream_of(stream_doc) else {
        return Err("This buffer isn't a review. Start one with :review".to_string());
    };
    if stream.scope == ChangeView::Staged {
        return Err("A staged review shows the index, not the files. ret opens the file.".into());
    }
    let doc = &editor.documents[&stream_doc];
    let text = doc.text().slice(..);
    let range = doc.selection(view).primary();
    let Some((file, line, column)) = place(stream, text, range.cursor(text)) else {
        return Err("The cursor isn't on an added or unchanged line".to_string());
    };
    // WHY: a code action asks about the range, so it carries over when it
    // stays on one file's rows. Goto and hover use only the cursor.
    let span = match (
        place(stream, text, range.from()),
        place(stream, text, range.to().saturating_sub(1)),
    ) {
        (Some((first_file, first, from)), Some((last_file, last, to)))
            if range.len() > 1 && first_file == file && last_file == file =>
        {
            Some(((first, from), (last, to)))
        }
        _ => None,
    };
    let path = stream.files[file].change.path().to_path_buf();
    let Content::Text { source, .. } = &stream.files[file].content else {
        unreachable!("only a text file has rows of lines");
    };
    let source = source.clone();
    let shown = helix_stdx::path::get_relative_path(&path);

    let open = editor.document_by_path(&path).is_some();
    let doc_id =
        load(editor, &path).map_err(|err| format!("Failed to open {}: {err}", shown.display()))?;
    let doc = editor.documents.get_mut(&doc_id).expect("just loaded");
    if !open && *doc.text() != source {
        return Err(format!(
            "{} changed on disk, so the review reads it again",
            shown.display()
        ));
    }

    let text = doc.text().slice(..);
    let at = |line: usize, column: usize| {
        let line = line.min(text.len_lines() - 1);
        (text.line_to_char(line) + column).min(line_end_char_index(&text, line))
    };
    let selection = match span {
        Some(((first, from), (last, to))) => {
            let (from, to) = (at(first, from), at(last, to) + 1);
            match range.head < range.anchor {
                true => Range::new(to, from),
                false => Range::new(from, to),
            }
        }
        _ => Range::point(at(line, column)),
    };
    doc.ensure_view_init(view);
    doc.set_selection(view, Selection::single(selection.anchor, selection.head));

    // WHY: upstream's commands see only servers that are up, and would say
    // there's no definition, or no server, while one starts.
    let starting = doc.language_servers.values().any(|ls| !ls.is_initialized());
    if starting && doc.language_servers().next().is_none() {
        return Err(format!(
            "{}'s language server is starting. Try again when it's ready",
            shown.display()
        ));
    }
    Ok(doc_id)
}
