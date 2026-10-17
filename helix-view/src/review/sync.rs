//! Forwards the stream's edits to the files, keeps its rows current, and sizes
//! its gutter. Runs before each key and each frame, so it catches an edit from
//! any source: a key, a typable command, a paste, a change to a file's buffer.

use std::path::{Path, PathBuf};

use helix_core::line_ending::get_line_ending;
use helix_core::{diff::compare_ropes, history::History, Rope, Transaction};

use helix_vcs::ChangeView;

use super::{apply_to_file, forward, layout, update, Content, Row, Stream};
use crate::document::{from_reader, Mode};
use crate::editor::Action;
use crate::tree::Tree;
use crate::{Document, DocumentId, Editor, ViewId};

/// `completing` is whether a completion menu is open.
pub fn sync(editor: &mut Editor, completing: bool) {
    let Some(doc_id) = editor.review.stream.as_ref().map(|stream| stream.doc) else {
        fit_gutters(editor);
        return;
    };
    if !editor.documents.contains_key(&doc_id) {
        editor.review.stream = None;
        fit_gutters(editor);
        return;
    }

    let taken = take_edits(editor, doc_id);
    let mut moved = taken.is_some();
    // WHY: a completion's preview restores a savepoint, which reverts every
    // edit since, a relayout too. A removed row it brought back would read as
    // text inserted into the file. The rows wait until the menu closes.
    if !completing {
        moved |= follow_sources(editor, doc_id);
        moved |= relayout(editor);
    }
    commit_files(editor, doc_id);
    if moved {
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
    if let Some(Err(message)) = taken {
        editor.set_status(message);
    }
    save_selections(editor, doc_id);
    fit_file_style(editor, doc_id);
    fit_gutters(editor);
}

// WHY: `o` and `Enter` insert the buffer's line ending, and `Tab` and `>` its
// indent. Both come from the file under the cursor, so what they insert is the
// file's own.
fn fit_file_style(editor: &mut Editor, doc_id: DocumentId) {
    let view = editor.tree.get(editor.tree.focus);
    let (Some(stream), Some(doc)) = (editor.review.stream.as_ref(), editor.documents.get(&doc_id))
    else {
        return;
    };
    if view.doc != doc_id {
        return;
    }
    let text = doc.text().slice(..);
    let row = text.char_to_line(doc.selection(view.id).primary().cursor(text));
    let Some(file) = stream.rows.get(row).map(|row| &stream.files[row.file()]) else {
        return;
    };
    // WHY: an open buffer's indent has its editorconfig and language in it.
    let indent = match editor.document_by_path(file.change.path()) {
        Some(doc) => doc.indent_style,
        None => file
            .indent(&editor.syn_loader.load())
            .unwrap_or(stream.indent),
    };
    let ending = file.ending();
    if let Some(doc) = editor.documents.get_mut(&doc_id) {
        doc.line_ending = ending;
        doc.indent_style = indent;
    }
}

/// The focused view if it shows `doc`, or else any view that does.
pub(super) fn view_of(editor: &Editor, doc: DocumentId) -> Option<ViewId> {
    let focus = editor.tree.get(editor.tree.focus);
    std::iter::once(focus)
        .chain(editor.tree.views().map(|(view, _)| view))
        .find(|view| view.doc == doc)
        .map(|view| view.id)
}

/// Forwards the stream's edits since the last sync to the files' buffers, or
/// puts them back and says why. `None` when there were none.
fn take_edits(editor: &mut Editor, doc_id: DocumentId) -> Option<Result<(), String>> {
    let stream = editor.review.stream.as_ref()?;
    if editor.documents.get(&doc_id)?.version() == stream.version {
        return None;
    }
    let staged = stream.scope == ChangeView::Staged;
    let refuse = |editor: &mut Editor, message: &str| {
        put_back(editor, doc_id);
        Some(Err(message.to_string()))
    };
    if staged {
        return refuse(editor, "A staged review is read-only. ret opens the file.");
    }
    // INVARIANT: the buffer's history is committed through a view of it.
    let Some(view) = view_of(editor, doc_id) else {
        return refuse(editor, "The edit was put back");
    };

    // WHY: the history holds exactly the edits since the last sync, which
    // cleared it. A diff of the text can't tell a deleted row from an equal
    // removed row next to it.
    let doc = editor.documents.get_mut(&doc_id).expect("checked above");
    doc.append_changes_to_history(editor.tree.get_mut(view));
    let Some(edit) = doc.history.get_mut().changes_since(0) else {
        // WHY: an edit undone before the sync leaves nothing to forward.
        settle(doc, &mut editor.tree, view);
        let version = doc.version();
        editor
            .review
            .stream
            .as_mut()
            .expect("checked above")
            .version = version;
        return None;
    };
    let stream = editor.review.stream.as_ref().expect("checked above");
    let forward = match forward::forward(stream, edit.changes()) {
        Ok(forward) => forward,
        Err(refusal) => return refuse(editor, refusal.message()),
    };
    let edits = forward.edits;

    let files: Vec<Edited> = edits
        .iter()
        .map(|edit| {
            let file = &stream.files[edit.file];
            let Content::Text { source, .. } = &file.content else {
                unreachable!("only a text file has rows to edit");
            };
            Edited {
                path: file.change.path().to_path_buf(),
                seen: file.seen,
                source: source.clone(),
            }
        })
        .collect();
    let docs = match buffers(editor, files) {
        Ok(docs) => docs,
        Err(message) => return refuse(editor, &message),
    };

    for (edit, doc) in edits.iter().zip(&docs) {
        let doc = editor.documents.get_mut(doc).expect("found above");
        let transaction = Transaction::change(doc.text(), edit.changes.iter().cloned());
        apply_to_file(doc, view, &transaction);
    }
    let doc = editor.documents.get_mut(&doc_id).expect("checked above");
    settle(doc, &mut editor.tree, view);
    let (text, version) = (doc.text().clone(), doc.version());
    let sources: Vec<_> = docs
        .iter()
        .map(|id| {
            let doc = &editor.documents[id];
            (doc.id(), doc.version(), doc.text().clone())
        })
        .collect();

    let stream = editor.review.stream.as_mut().expect("checked above");
    stream.ranges.map(edit.changes());
    debug_assert_eq!(forward.text, text);
    stream.text = text;
    stream.version = version;
    stream.rows = forward.rows;
    for (edit, (id, version, text)) in edits.iter().zip(sources) {
        let file = &mut stream.files[edit.file];
        if let Content::Text { source, .. } = &mut file.content {
            file.opened =
                layout::map_ranges(&file.opened, layout::line_map(source, &text).as_deref());
            *source = text;
        }
        // WHY: the relayout compares the file with itself, so it keeps the
        // parse and hunks of the old text unless they go here.
        file.trees.source = Default::default();
        file.hunks = Default::default();
        file.indent = Default::default();
        file.seen = Some((id, version));
        if !stream.pending.iter().any(|&(doc, _)| doc == id) {
            stream.pending.push((id, view));
        }
    }
    stream.relayout = true;
    // INVARIANT: the forwarded edit made each file what its rows show.
    debug_assert!(rows_match_sources(stream));
    Some(Ok(()))
}

/// A file an edit goes to, and what its rows came from.
struct Edited {
    path: PathBuf,
    seen: Option<(DocumentId, i32)>,
    source: Rope,
}

/// The buffer of each edited file, loaded if it isn't open, or why the edit
/// can't go to one.
fn buffers(editor: &mut Editor, files: Vec<Edited>) -> Result<Vec<DocumentId>, String> {
    let mut docs = Vec::with_capacity(files.len());
    for Edited { path, seen, source } in files {
        let shown = helix_stdx::path::get_relative_path(&path);
        let doc = match editor.document_by_path(&path) {
            Some(doc) if Some((doc.id(), doc.version())) == seen => doc.id(),
            // WHY: the rows came from an older version of the buffer, so the
            // edit's places in it are stale.
            Some(_) => {
                return Err(format!(
                    "{} changed, so the edit was put back",
                    shown.display()
                ))
            }
            None => match editor.open(&path, Action::Load) {
                Ok(doc) if *editor.documents[&doc].text() == source => doc,
                Ok(_) => {
                    return Err(format!(
                        "{} changed on disk, so the edit was put back",
                        shown.display()
                    ))
                }
                Err(err) => return Err(format!("Failed to open {}: {err}", shown.display())),
            },
        };
        docs.push(doc);
    }
    Ok(docs)
}

fn rows_match_sources(stream: &Stream) -> bool {
    let one_per_line = stream.rows.len() + 1 == stream.text.len_lines();
    one_per_line
        && stream.rows.iter().enumerate().all(|(row, kind)| {
            let (Row::Context { file, source, .. } | Row::Added { file, source }) = *kind else {
                return true;
            };
            let Content::Text { source: rope, .. } = &stream.files[file].content else {
                return false;
            };
            if source >= layout::line_count(rope) {
                return false;
            }
            let (shown, line) = (stream.text.line(row), rope.line(source));
            match get_line_ending(&line) {
                Some(_) => shown == line,
                // WHY: a last line with no line ending shows one.
                None => get_line_ending(&shown).is_some_and(|ending| {
                    shown.slice(..shown.len_chars() - ending.len_chars()) == line
                }),
            }
        })
}

fn relayout(editor: &mut Editor) -> bool {
    let Some(stream) = editor
        .review
        .stream
        .as_ref()
        .filter(|stream| stream.relayout)
    else {
        return false;
    };
    let files = stream.files.clone();
    update(editor, files, None);
    true
}

// WHY: one command in the review, or one insert session, is one undo step in
// each file it edited.
fn commit_files(editor: &mut Editor, doc_id: DocumentId) {
    let focus = editor.tree.focus;
    if editor.mode == Mode::Insert && editor.tree.get(focus).doc == doc_id {
        return;
    }
    let Some(stream) = editor.review.stream.as_mut() else {
        return;
    };
    for (doc, view) in std::mem::take(&mut stream.pending) {
        let Some(doc) = editor.documents.get_mut(&doc) else {
            continue;
        };
        let view = match editor.tree.contains(view) {
            true => view,
            false => focus,
        };
        doc.ensure_view_init(view);
        doc.append_changes_to_history(editor.tree.get_mut(view));
    }
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
    let (Some(stream), Some(doc)) = (editor.review.stream.as_ref(), editor.documents.get(&doc_id))
    else {
        return false;
    };
    if doc.version() == stream.version {
        return false;
    }
    if editor.mode == Mode::Insert && editor.tree.get(editor.tree.focus).doc == doc_id {
        // INVARIANT: before the text is restored. Leaving insert mode can
        // edit the buffer, to drop the indent of an empty line.
        editor.enter_normal_mode();
    }

    let (Some(stream), Some(doc)) = (
        editor.review.stream.as_mut(),
        editor.documents.get_mut(&doc_id),
    ) else {
        return false;
    };
    let transaction = compare_ropes(doc.text(), &stream.text);
    doc.apply(&transaction, editor.tree.focus);

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
    if stream.scope == ChangeView::Staged {
        return false;
    }
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
