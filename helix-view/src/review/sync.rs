//! Keeps the stream read-only, and sizes its gutter. Runs before each frame,
//! so it catches an edit from any source: a key, a typable command, a paste.

use helix_core::{diff::compare_ropes, history::History, Transaction};

use crate::document::Mode;
use crate::tree::Tree;
use crate::{Document, Editor, ViewId};

pub fn sync(editor: &mut Editor) {
    fit_gutters(editor);
    let Some(doc_id) = editor.review.stream.as_ref().map(|stream| stream.doc) else {
        return;
    };
    if !editor.documents.contains_key(&doc_id) {
        editor.review.stream = None;
        return;
    }

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
        return;
    };
    if doc.version() != stream.version {
        let transaction = compare_ropes(doc.text(), &stream.text);
        doc.apply(&transaction, editor.tree.focus);
        refused = true;
    }

    let mut shown_in = None;
    for (view, _) in editor.tree.views_mut() {
        if view.doc != doc_id {
            continue;
        }
        shown_in.get_or_insert(view.id);
        if !refused {
            stream
                .selections
                .insert(view.id, doc.selection(view.id).clone());
        } else if let Some(selection) = stream.selections.get(&view.id) {
            doc.set_selection(view.id, selection.clone());
        }
    }
    if !refused {
        return;
    }
    if let Some(view) = shown_in {
        settle(doc, &mut editor.tree, view);
    }
    stream.version = doc.version();

    let scrolloff = editor.config().scrolloff;
    let focus = editor.tree.focus;
    if editor.tree.get(focus).doc == doc_id {
        let doc = editor.documents.get_mut(&doc_id).expect("checked above");
        editor
            .tree
            .get_mut(focus)
            .ensure_cursor_in_view(doc, scrolloff);
    }
    editor.set_status("The review is read-only. ret opens the file.");
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
