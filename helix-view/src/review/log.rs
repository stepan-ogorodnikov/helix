//! The review's undo log. The stream's own history is cleared on every sync,
//! so `u` and `U` in the review walk this log instead. A step is one command in
//! the review, or one insert session: the revisions it left in each file's
//! buffer. Undo and redo run in the files' own histories.

use helix_core::line_ending::line_end_char_index;
use helix_core::{Range, Selection};
use helix_vcs::ChangeView;

use super::{sync, Row};
use crate::{Document, DocumentId, Editor, ViewId};

#[derive(Default)]
pub(super) struct Log {
    undo: Vec<Vec<Commit>>,
    redo: Vec<Vec<Commit>>,
}

impl Log {
    pub(super) fn push(&mut self, step: Vec<Commit>) {
        if step.is_empty() {
            return;
        }
        self.undo.push(step);
        self.redo.clear();
    }

    /// The steps `way` walks, then the ones it leaves behind.
    fn stacks(&mut self, way: Way) -> (&mut Vec<Vec<Commit>>, &mut Vec<Vec<Commit>>) {
        match way {
            Way::Undo => (&mut self.undo, &mut self.redo),
            Way::Redo => (&mut self.redo, &mut self.undo),
        }
    }
}

/// One file's revision in a step.
pub(super) struct Commit {
    doc: DocumentId,
    /// The buffer's name, for a message after it closes.
    name: String,
    before: usize,
    after: usize,
}

impl Commit {
    /// The revision `doc` committed since `before`, if it committed one.
    pub(super) fn since(doc: &mut Document, before: usize) -> Option<Commit> {
        let after = doc.get_current_revision();
        (after != before).then(|| Commit {
            doc: doc.id(),
            name: doc.display_name().into_owned(),
            before,
            after,
        })
    }

    /// The revision `way` starts from, and the one it lands on.
    fn ends(&self, way: Way) -> (usize, usize) {
        match way {
            Way::Undo => (self.after, self.before),
            Way::Redo => (self.before, self.after),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Undo,
    Redo,
}

pub fn undo(editor: &mut Editor, count: usize) {
    walk(editor, count, Way::Undo);
}

pub fn redo(editor: &mut Editor, count: usize) {
    walk(editor, count, Way::Redo);
}

/// Undoes or redoes `count` steps in the review the focused view shows, and
/// puts a cursor where each file of the last one changed.
fn walk(editor: &mut Editor, count: usize, way: Way) {
    let view = editor.tree.focus;
    let Some(stream) = editor.review.stream_of(editor.tree.get(view).doc) else {
        return;
    };
    let doc_id = stream.doc;
    if stream.scope == ChangeView::Staged {
        editor.set_status("A staged review is read-only. ret opens the file.");
        return;
    }
    // WHY: upstream's `undo` commits first, so `u` on a key bound in insert
    // mode undoes the insert so far.
    sync::commit_pending(editor);

    let mut moved = Vec::new();
    let mut message = None;
    for _ in 0..count {
        let log = &mut editor.review.stream.as_mut().expect("checked above").log;
        let Some(step) = log.stacks(way).0.pop() else {
            message = Some(match way {
                Way::Undo => "Already at oldest change".to_string(),
                Way::Redo => "Already at newest change".to_string(),
            });
            break;
        };
        let walked = walk_step(editor, &step, view, way);
        let log = &mut editor.review.stream.as_mut().expect("checked above").log;
        let (from, to) = log.stacks(way);
        match walked {
            Ok(()) => {
                moved = step.iter().map(|commit| commit.doc).collect();
                to.push(step);
            }
            Err(refusal) => {
                from.push(step);
                message = Some(refusal);
                break;
            }
        }
    }

    if !moved.is_empty() {
        sync::follow_sources(editor, doc_id);
        put_cursors(editor, doc_id, view, &moved);
        sync::save_selections(editor, doc_id);
    }
    if let Some(message) = message {
        editor.set_status(message);
    }
}

/// Walks one step in each of its files, or in none of them and says why.
fn walk_step(editor: &mut Editor, step: &[Commit], view: ViewId, way: Way) -> Result<(), String> {
    let refusal = |commit: &Commit, why: &str| {
        let done = match way {
            Way::Undo => "undone",
            Way::Redo => "redone",
        };
        format!("{} {why}, so nothing was {done}", commit.name)
    };
    let moved_on = match way {
        Way::Undo => "changed after this edit",
        Way::Redo => "changed after the undo",
    };
    for commit in step {
        let Some(doc) = editor.documents.get_mut(&commit.doc) else {
            return Err(refusal(commit, "is closed"));
        };
        // WHY: a change not yet committed would be committed by the undo and
        // undone in place of the step.
        if !doc.changes().is_empty() || doc.get_current_revision() != commit.ends(way).0 {
            return Err(refusal(commit, moved_on));
        }
    }

    for (walked, commit) in step.iter().enumerate() {
        let doc = editor
            .documents
            .get_mut(&commit.doc)
            .expect("checked above");
        doc.ensure_view_init(view);
        let view = editor.tree.get_mut(view);
        match way {
            Way::Undo => doc.undo(view),
            Way::Redo => doc.redo(view),
        };
        if doc.get_current_revision() == commit.ends(way).1 {
            continue;
        }
        // WHY: `History` doesn't say which branch a redo takes until it runs.
        // This one took a newer branch, made in the file after the undo.
        for commit in step[..=walked].iter().rev() {
            let doc = editor
                .documents
                .get_mut(&commit.doc)
                .expect("checked above");
            doc.undo(view);
        }
        return Err(refusal(commit, moved_on));
    }
    Ok(())
}

/// Puts a cursor on each file's cursor in the stream's `view`: on that line's
/// row, or on the file's header when the line isn't shown.
fn put_cursors(editor: &mut Editor, doc_id: DocumentId, view: ViewId, files: &[DocumentId]) {
    let Some(stream) = editor.review.stream.as_ref() else {
        return;
    };
    let text = editor.documents[&doc_id].text().slice(..);
    let mut points: Vec<usize> = files
        .iter()
        .filter_map(|&id| {
            let file = editor.documents.get(&id)?;
            let index = stream.files.iter().position(|shown| {
                editor
                    .document_by_path(shown.change.path())
                    .is_some_and(|doc| doc.id() == id)
            })?;
            let source = file.text().slice(..);
            let cursor = file.selections().get(&view)?.primary().cursor(source);
            let line = source.char_to_line(cursor);
            let column = cursor - source.line_to_char(line);
            let row = stream.rows.iter().position(|row| match *row {
                Row::Context { file, source, .. } | Row::Added { file, source } => {
                    file == index && source == line
                }
                _ => false,
            });
            match row {
                Some(row) => {
                    Some((text.line_to_char(row) + column).min(line_end_char_index(&text, row)))
                }
                None => stream
                    .rows
                    .iter()
                    .position(|row| *row == Row::Header { file: index })
                    .map(|row| text.line_to_char(row)),
            }
        })
        .collect();
    if points.is_empty() {
        return;
    }
    points.sort_unstable();
    points.dedup();
    let selection = Selection::new(points.into_iter().map(Range::point).collect(), 0);
    let scrolloff = editor.config().scrolloff;
    let doc = editor.documents.get_mut(&doc_id).expect("checked above");
    doc.set_selection(view, selection);
    editor
        .tree
        .get_mut(view)
        .ensure_cursor_in_view(doc, scrolloff);
}
