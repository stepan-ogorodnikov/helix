//! Keeps the stream read-only and current, and sizes its gutter. Runs before
//! each frame, so it catches an edit from any source: a key, a typable command,
//! a paste, a change to a file's buffer.

use std::path::Path;

use helix_core::{diff::compare_ropes, history::History, Rope, Transaction};

use super::{update, Content};
use crate::document::{from_reader, Mode};
use crate::tree::Tree;
use crate::{Document, DocumentId, Editor, ViewId};

pub fn sync(editor: &mut Editor) {
    let Some(doc_id) = editor.review.stream.as_ref().map(|stream| stream.doc) else {
        fit_gutters(editor);
        return;
    };
    if !editor.documents.contains_key(&doc_id) {
        editor.review.stream = None;
        fit_gutters(editor);
        return;
    }

    let refused = put_back(editor, doc_id);
    let followed = follow_sources(editor, doc_id);
    if refused || followed {
        let scrolloff = editor.config().scrolloff;
        let focus = editor.tree.focus;
        if editor.tree.get(focus).doc == doc_id {
            let doc = editor.documents.get_mut(&doc_id).expect("checked above");
            editor
                .tree
                .get_mut(focus)
                .ensure_cursor_in_view(doc, scrolloff);
        }
    }
    if refused {
        self::refused(editor);
    }
    save_selections(editor, doc_id);
    fit_gutters(editor);
}

pub(super) fn refused(editor: &mut Editor) {
    editor.set_status("The review is read-only. ret opens the file.");
}

pub(super) fn save_selections(editor: &mut Editor, doc_id: DocumentId) {
    let (Some(stream), Some(doc)) = (editor.review.stream.as_mut(), editor.documents.get(&doc_id))
    else {
        return;
    };
    for (view, _) in editor.tree.views() {
        if view.doc == doc_id {
            stream
                .selections
                .insert(view.id, doc.selection(view.id).clone());
        }
    }
}

pub(super) fn put_back(editor: &mut Editor, doc_id: DocumentId) -> bool {
    let mut refused = false;
    if editor.mode == Mode::Insert && editor.tree.get(editor.tree.focus).doc == doc_id {
        // INVARIANT: before the text is restored. Leaving insert mode can
        // edit the buffer, to drop the indent of an empty line.
        editor.enter_normal_mode();
        refused = true;
    }

    let (Some(stream), Some(doc)) = (
        editor.review.stream.as_mut(),
        editor.documents.get_mut(&doc_id),
    ) else {
        return false;
    };
    if doc.version() != stream.version {
        let transaction = compare_ropes(doc.text(), &stream.text);
        doc.apply(&transaction, editor.tree.focus);
        refused = true;
    }
    if !refused {
        return false;
    }

    let mut shown_in = None;
    for (view, _) in editor.tree.views() {
        if view.doc != doc_id {
            continue;
        }
        shown_in.get_or_insert(view.id);
        if let Some(selection) = stream.selections.get(&view.id) {
            doc.set_selection(view.id, selection.clone());
        }
    }
    if let Some(view) = shown_in {
        settle(doc, &mut editor.tree, view);
    }
    stream.version = doc.version();
    true
}

fn follow_sources(editor: &mut Editor, doc_id: DocumentId) -> bool {
    // WHY: a hidden stream waits. `update` commits through a view of it, and
    // nothing shows the rows until one does.
    if !editor.tree.views().any(|(view, _)| view.doc == doc_id) {
        return false;
    }
    let Some(stream) = editor.review.stream.as_ref() else {
        return false;
    };
    let mut files = None;
    for (index, file) in stream.files.iter().enumerate() {
        let Content::Text { base, .. } = &file.content else {
            continue;
        };
        let doc = editor.document_by_path(file.change.path());
        if doc.map(|doc| (doc.id(), doc.version())) == file.seen {
            continue;
        }
        let source = match doc {
            Some(doc) => doc.text().clone(),
            None => read(file.change.path()),
        };
        let files = files.get_or_insert_with(|| stream.files.clone());
        files[index].content = Content::Text {
            base: base.clone(),
            source,
        };
    }
    let Some(files) = files else {
        return false;
    };
    update(editor, files, None);
    true
}

fn read(path: &Path) -> Rope {
    std::fs::File::open(path)
        .and_then(|mut file| from_reader(&mut file, None))
        .map(|(text, ..)| text)
        .unwrap_or_default()
}

// WHY: the stream keeps no history. A modified stream would hold up `:q`, and
// `u` would bring back a refused edit.
pub(super) fn settle(doc: &mut Document, tree: &mut Tree, view: ViewId) {
    doc.append_changes_to_history(tree.get_mut(view));
    // INVARIANT: every view maps its jumps through the old history before it
    // goes, then records revision 0. A view left at a later revision panics in
    // `History::changes_since`.
    for (view, _) in tree.views_mut() {
        view.sync_changes(doc);
    }
    doc.history.set(History::default());
    let unchanged = Transaction::new(doc.text());
    for (view, _) in tree.views_mut() {
        view.apply(&unchanged, doc);
    }
    doc.reset_modified();
}

// WORKAROUND: `GutterType::width` gets no `&Editor`, so the gutter can't size
// itself from the rows. This goes if upstream passes the editor there.
fn fit_gutters(editor: &mut Editor) {
    let stream = editor
        .review
        .stream
        .as_ref()
        .map(|stream| (stream.doc, stream.gutter_width()));
    if stream.is_none() && !editor.review.widened {
        return;
    }
    let configured = editor.config().gutters.line_numbers.min_width;
    for (view, _) in editor.tree.views_mut() {
        view.gutters.line_numbers.min_width = match stream {
            Some((doc, width)) if view.doc == doc => width,
            _ => configured,
        };
    }
    editor.review.widened = stream.is_some();
}
