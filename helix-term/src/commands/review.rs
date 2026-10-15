//! FORK: diff review. The per-view inline diff toggle, and review sessions that
//! step through the changed files of a repository and their hunks.

use std::path::Path;

use anyhow::anyhow;
use helix_core::command_line::Args;
use helix_core::fuzzy::fuzzy_match;
use helix_core::movement::Direction;
use helix_core::{Rope, Selection};
use helix_loader::workspace_trust::TrustQuery;
use helix_vcs::{index, visible_changes, ChangeKind, ChangeView, FileChange, Hunk};
use helix_view::document::from_reader;
use helix_view::editor::Action;
use helix_view::review::{ReviewBase, ReviewSession};
use helix_view::{align_view, current, current_ref, doc, doc_mut, view_mut, Align, Editor};
use imara_diff::{Algorithm, Diff, IndentHeuristic, IndentLevel, InternedInput, Interner};

use super::{hunk_range, push_jump, Context};
use crate::compositor;
use crate::job::Callback;
use crate::ui::prompt::Completion;
use crate::ui::PromptEvent;

pub(super) fn toggle_inline_diff(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    view.inline_diff.enabled = !view.inline_diff.enabled;
    if view.inline_diff.enabled && doc.diff_handle().is_none() {
        cx.editor
            .set_status("Inline diff is on, but this file has no diff base");
    }
}

const LISTS: [ChangeView; 3] = [ChangeView::Unstaged, ChangeView::Staged, ChangeView::All];

/// `:review [unstaged|staged|all]`. Lists the changes of the repository that holds
/// the current file, or the working directory for a buffer with no path.
/// `unstaged` diffs against the index, the others against `HEAD`.
pub(super) fn review(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let list = match args.first() {
        None => ChangeView::All,
        Some(name) => LISTS
            .into_iter()
            .find(|list| list.label() == name)
            .ok_or_else(|| anyhow!("'{name}' is not one of unstaged, staged, or all"))?,
    };
    let root = doc!(cx.editor)
        .path()
        .and_then(Path::parent)
        .filter(|dir| dir.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(helix_stdx::env::current_working_dir);
    let trust_full = cx
        .editor
        .workspace_trust
        .query(&helix_loader::find_workspace_in(&root).0, TrustQuery::Git)
        .is_trusted();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    cx.editor
        .diff_providers
        .clone()
        .for_each_changed_file(root, trust_full, move |change| tx.send(change).is_ok());
    cx.jobs.callback(async move {
        let (mut unstaged, mut staged, mut error) = (Vec::new(), Vec::new(), None);
        while let Some(change) = rx.recv().await {
            match change {
                Ok((ChangeKind::Unstaged, change)) => unstaged.push(change),
                Ok((ChangeKind::Staged, change)) => staged.push(change),
                Err(err) => {
                    error.get_or_insert(err);
                }
            }
        }
        let mut files = visible_changes(list, &unstaged, &staged);
        files.sort_by(|a, b| a.path().cmp(b.path()));
        let start = move |editor: &mut Editor| match error {
            Some(err) if files.is_empty() => {
                editor.set_error(format!("Failed to list the changed files: {err}"))
            }
            _ => start_review(editor, files, 0, base_for(list)),
        };
        Ok(Callback::Editor(Box::new(start)))
    });
    Ok(())
}

pub(super) fn complete_list(_editor: &Editor, input: &str) -> Vec<Completion> {
    fuzzy_match(input, LISTS.iter().map(|list| list.label()), false)
        .into_iter()
        .map(|(label, _)| ((0..), label.into()))
        .collect()
}

/// What a review of `list` diffs against. A buffer holds worktree text, so a
/// staged-only diff can't be drawn on it, and `staged` uses `HEAD` too.
pub(super) fn base_for(list: ChangeView) -> ReviewBase {
    match list {
        ChangeView::Unstaged => ReviewBase::Index,
        ChangeView::Staged | ChangeView::All => ReviewBase::Head,
    }
}

/// Starts a review of `files` in the focused view, at `files[index]`.
pub(super) fn start_review(
    editor: &mut Editor,
    files: Vec<FileChange>,
    index: usize,
    base: ReviewBase,
) {
    if files.is_empty() {
        editor.set_status("There are no changes to review");
        return;
    }
    let index = index.min(files.len() - 1);
    let view = view_mut!(editor);
    view.inline_diff.enabled = true;
    view.review = Some(ReviewSession {
        files,
        current: index,
        doc: None,
        base,
    });
    open_file(editor, index, Landing::FirstHunk);
}

pub(super) fn next_file(cx: &mut Context) {
    step_file(cx.editor, cx.count(), Direction::Forward);
}

pub(super) fn prev_file(cx: &mut Context) {
    step_file(cx.editor, cx.count(), Direction::Backward);
}

fn step_file(editor: &mut Editor, count: usize, direction: Direction) {
    let (view, _) = current_ref!(editor);
    let Some(session) = &view.review else {
        editor.set_error("This view has no review. Start one with :review");
        return;
    };
    let target = match direction {
        Direction::Forward => (session.current + count).min(session.files.len() - 1),
        Direction::Backward => session.current.saturating_sub(count),
    };
    if target == session.current && session.doc == Some(view.doc) {
        editor.set_status(match direction {
            Direction::Forward => "This is the last file in the review",
            Direction::Backward => "This is the first file in the review",
        });
        return;
    }
    open_file(editor, target, Landing::FirstHunk);
}

pub(super) fn next_hunk(cx: &mut Context) {
    step_hunk(cx, Direction::Forward);
}

pub(super) fn prev_hunk(cx: &mut Context) {
    step_hunk(cx, Direction::Backward);
}

fn step_hunk(cx: &mut Context, direction: Direction) {
    let count = cx.count();
    cx.editor.apply_motion(move |editor| {
        for _ in 0..count {
            if !hunk_step(editor, direction) {
                break;
            }
        }
    });
}

/// Selects the next hunk in `direction`, like `]g` and `[g`. Past the last hunk
/// of the review's current file, opens the neighboring file of the review.
/// Returns whether the selection moved.
fn hunk_step(editor: &mut Editor, direction: Direction) -> bool {
    let (view, doc) = current!(editor);
    let text = doc.text().slice(..);
    let line = doc.selection(view.id).primary().cursor_line(text) as u32;
    let hunk = doc.diff_handle().and_then(|handle| {
        let diff = handle.load();
        let index = match direction {
            Direction::Forward => diff.next_hunk(line),
            Direction::Backward => diff.prev_hunk(line),
        }?;
        Some(diff.nth_hunk(index))
    });
    if let Some(hunk) = hunk {
        let range = hunk_range(hunk, text).with_direction(direction);
        push_jump(view, doc);
        doc.set_selection(view.id, Selection::single(range.anchor, range.head));
        return true;
    }

    // Only cross files from the file the review is on.
    let Some(session) = &view.review else {
        return false;
    };
    if session.doc != Some(view.doc) {
        return false;
    }
    let (target, landing) = match direction {
        Direction::Forward => (
            Some(session.current + 1).filter(|&next| next < session.files.len()),
            Landing::FirstHunk,
        ),
        Direction::Backward => (session.current.checked_sub(1), Landing::LastHunk),
    };
    let Some(target) = target else {
        editor.set_status("There are no more changes in the review");
        return false;
    };
    open_file(editor, target, landing);
    true
}

#[derive(Clone, Copy)]
enum Landing {
    FirstHunk,
    LastHunk,
}

/// Opens `files[index]` of the focused view's review, refreshes its diff base,
/// and selects its first or last hunk.
fn open_file(editor: &mut Editor, index: usize, landing: Landing) {
    let Some(session) = &mut view_mut!(editor).review else {
        return;
    };
    session.current = index;
    session.doc = None;
    let change = session.files[index].clone();
    let base = session.base;
    let position = format!("Review {}/{}", index + 1, session.files.len());

    let doc_id = match editor.open(change.path(), Action::Replace) {
        Ok(id) => id,
        Err(err) => {
            editor.set_error(format!("{position}: {}: {err}", describe(&change)));
            return;
        }
    };
    let base = refresh_base(editor, &change, base);

    let (view, doc) = current!(editor);
    let text = doc.text();
    let hunks = base.map(|base| diff_hunks(&base, text)).unwrap_or_default();
    let hunk = match landing {
        Landing::FirstHunk => hunks.first(),
        Landing::LastHunk => hunks.last(),
    };
    if let Some(hunk) = hunk {
        let range = hunk_range(hunk.clone(), text.slice(..));
        doc.set_selection(view.id, Selection::single(range.anchor, range.head));
    }
    align_view(doc, view, Align::Center);
    if let Some(session) = &mut view.review {
        session.doc = Some(doc_id);
    }
    editor.set_status(format!("{position}: {}", describe(&change)));
}

/// Loads the `base` version of `change` as the diff base of the focused
/// document, and returns it. The base only loads on open and reload, so it is
/// stale after a commit. A file with no such version gets an empty base.
fn refresh_base(editor: &mut Editor, change: &FileChange, base: ReviewBase) -> Option<Rope> {
    let trust_full = editor
        .workspace_trust
        .query(doc!(editor).workspace_root(), TrustQuery::Git)
        .is_trusted();
    let base_path = match change {
        FileChange::Renamed { from_path, .. } => from_path,
        _ => change.path(),
    };
    let bytes = match (base, change) {
        // A conflicted file has no unconflicted index version.
        (ReviewBase::Head, _) | (ReviewBase::Index, FileChange::Conflict { .. }) => {
            editor.diff_providers.get_diff_base(base_path, trust_full)
        }
        (ReviewBase::Index, _) => index::get_index_base(base_path, trust_full)
            .inspect_err(|err| log::debug!("failed to read the index base: {err:#}"))
            .ok()
            .flatten(),
    }
    .unwrap_or_default();

    let doc = doc_mut!(editor);
    let (base, ..) = from_reader(&mut bytes.as_slice(), Some(doc.encoding())).ok()?;
    if doc
        .diff_handle()
        .is_none_or(|handle| *handle.load().diff_base() != base)
    {
        doc.set_diff_base(bytes);
    }
    Some(base)
}

/// Reloads the focused document's base after its index version changed, when
/// the focused view reviews it against the index.
pub(super) fn index_changed(editor: &mut Editor) {
    let (view, _) = current_ref!(editor);
    let Some(session) = &view.review else {
        return;
    };
    if session.base == ReviewBase::Index && session.doc == Some(view.doc) {
        let change = session.files[session.current].clone();
        refresh_base(editor, &change, ReviewBase::Index);
    }
}

/// The diff worker's size limits. Past them it reports no hunks.
const MAX_DIFF_LINES: usize = 64 * u16::MAX as usize;
const MAX_DIFF_BYTES: usize = MAX_DIFF_LINES * 128;

/// The hunks of `doc` against `base`, computed the way the diff worker does.
/// A file switch can't wait for the worker, which runs in the background.
pub(super) fn diff_hunks(base: &Rope, doc: &Rope) -> Vec<Hunk> {
    let too_large =
        |text: &Rope| text.len_lines() > MAX_DIFF_LINES || text.len_bytes() > MAX_DIFF_BYTES;
    if too_large(base) || too_large(doc) {
        return Vec::new();
    }
    let mut input = InternedInput {
        before: Vec::new(),
        after: Vec::new(),
        interner: Interner::new(base.len_lines() + doc.len_lines()),
    };
    input.update_before(base.lines());
    input.update_after(doc.lines());
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_with(
        &input.before,
        &input.after,
        IndentHeuristic::new(|token| IndentLevel::for_ascii_line(input.interner[token].bytes(), 4)),
    );
    diff.hunks().collect()
}

/// The path of `change` relative to the working directory, and how it changed
/// when the text alone doesn't show it.
fn describe(change: &FileChange) -> String {
    let show = |path: &Path| {
        helix_stdx::path::get_relative_path(path)
            .display()
            .to_string()
    };
    match change {
        FileChange::Modified { path } => show(path),
        FileChange::Untracked { path } | FileChange::Added { path } => {
            format!("{} (new)", show(path))
        }
        FileChange::Deleted { path } => format!("{} (deleted)", show(path)),
        FileChange::Conflict { path } => format!("{} (conflict)", show(path)),
        FileChange::Renamed { from_path, to_path } => {
            format!("{} (renamed from {})", show(to_path), show(from_path))
        }
    }
}

#[cfg(test)]
mod test {
    use helix_core::Rope;
    use helix_vcs::Hunk;

    use super::diff_hunks;

    #[test]
    fn diff_hunks_match_the_diff_worker() {
        let base = Rope::from("a\nb\nc\nd\n");
        let doc = Rope::from("a\nB\nc\nd\ne\n");
        assert_eq!(
            diff_hunks(&base, &doc),
            [
                Hunk {
                    before: 1..2,
                    after: 1..2,
                },
                Hunk {
                    before: 4..4,
                    after: 4..5,
                },
            ]
        );
    }

    #[test]
    fn an_empty_base_makes_one_added_hunk() {
        let doc = Rope::from("one\ntwo\n");
        assert_eq!(
            diff_hunks(&Rope::new(), &doc),
            [Hunk {
                before: 0..0,
                after: 0..2,
            }]
        );
    }
}
