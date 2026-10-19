//! `:review` and the commands of the review stream. The rows and the buffer
//! live in `helix_view::review`. This module lists the changed files, reads
//! both sides of each, and opens a row's file.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _};
use helix_core::command_line::Args;
use helix_core::fuzzy::fuzzy_match;
use helix_core::line_ending::line_end_char_index;
use helix_core::movement::Direction;
use helix_core::{encoding, Range, Rope, RopeSlice, Selection};
use helix_loader::workspace_trust::TrustQuery;
use helix_vcs::{index, visible_changes, ChangeKind, ChangeView, DiffProviderRegistry, FileChange};
use helix_view::document::from_reader;
use helix_view::document::Mode;
use helix_view::editor::Action;
use helix_view::review::{self, Content, Row, Section, Stream};
use helix_view::{align_view, current, current_ref, doc, view, Align, Editor};

mod lsp;
mod restore;
mod stage;

pub(crate) use lsp::lent;
pub(super) use restore::review_restore;
pub(super) use stage::{stage_hunk, unstage_hunk};

use super::{push_jump, Context};
use crate::compositor;
use crate::job::Callback;
use crate::ui::prompt::Completion;
use crate::ui::PromptEvent;

const SCOPES: [ChangeView; 3] = [ChangeView::All, ChangeView::Unstaged, ChangeView::Staged];

pub(super) fn review(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let scope = match args.first() {
        None => None,
        Some(name) => Some(
            SCOPES
                .into_iter()
                .find(|scope| scope.label() == name)
                .ok_or_else(|| anyhow!("'{name}' is not one of all, unstaged, or staged"))?,
        ),
    };
    let open = open_stream(cx.editor).map(|stream| (stream.doc, stream.scope));
    match (open, scope) {
        (Some((doc, open)), scope) if scope.is_none_or(|scope| scope == open) => {
            cx.editor.switch(doc, Action::Replace);
            cx.editor
                .set_status(format!("Reviewing {} changes", open.label()));
        }
        (_, scope) => read_changes(cx, scope.unwrap_or(ChangeView::All), None, None),
    }
    Ok(())
}

pub(super) fn complete_scope(_editor: &Editor, input: &str) -> Vec<Completion> {
    fuzzy_match(input, SCOPES.iter().map(|scope| scope.label()), false)
        .into_iter()
        .map(|(label, _)| ((0..), label.into()))
        .collect()
}

pub(super) fn review_refresh(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        let scope = open_stream(cx.editor).map_or(ChangeView::All, |stream| stream.scope);
        read_changes(cx, scope, None, None);
    }
    Ok(())
}

pub(super) fn review_context(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let context = open_stream(cx.editor)
        .context("There is no review. Start one with :review")?
        .context;
    let Some(lines) = args.first() else {
        let plural = if context == 1 { "" } else { "s" };
        cx.editor.set_status(format!(
            "The review shows {context} line{plural} of context"
        ));
        return Ok(());
    };
    let lines = lines
        .parse()
        .map_err(|_| anyhow!("'{lines}' is not a number of lines"))?;
    review::set_context(cx.editor, lines);
    Ok(())
}

fn open_stream(editor: &Editor) -> Option<&Stream> {
    editor
        .review
        .stream()
        .filter(|stream| editor.documents.contains_key(&stream.doc))
}

/// The changed-file picker's `review_jump`: the open review at `path`'s header
/// when it shows `scope`'s changes and has the file, or else a new read of
/// `scope`'s changes, then the header.
pub(super) fn jump(cx: &mut compositor::Context, scope: ChangeView, path: &Path) {
    if let Some(doc) = open_stream(cx.editor)
        .filter(|stream| stream.scope == scope)
        .map(|stream| stream.doc)
    {
        cx.editor.switch(doc, Action::Replace);
        if review::focus_file(cx.editor, path) {
            return;
        }
    }
    read_changes(cx, scope, None, Some(path.to_path_buf()));
}

/// Lists the `scope` changes of the repository the open review shows, or else
/// the one that holds the current file, or the working directory for a buffer
/// with no path. Shows them in the stream, then `done` in the status line, and
/// puts the cursor on `focus`'s header.
fn read_changes(
    cx: &mut compositor::Context,
    scope: ChangeView,
    done: Option<String>,
    focus: Option<PathBuf>,
) {
    let root = match open_stream(cx.editor) {
        Some(stream) => stream.root.clone(),
        None => doc!(cx.editor)
            .path()
            .and_then(Path::parent)
            .filter(|dir| dir.is_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(helix_stdx::env::current_working_dir),
    };
    let trust_full = cx
        .editor
        .workspace_trust
        .query(&helix_loader::find_workspace_in(&root).0, TrustQuery::Git)
        .is_trusted();
    let providers = cx.editor.diff_providers.clone();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    providers
        .clone()
        .for_each_changed_file(root.clone(), trust_full, move |change| {
            tx.send(change).is_ok()
        });
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
        let mut changes = visible_changes(scope, &unstaged, &staged);
        changes.sort_by(|a, b| a.path().cmp(b.path()));
        let files = tokio::task::spawn_blocking(move || {
            changes
                .into_iter()
                .map(|change| File::read(&providers, scope, change, trust_full))
                .collect::<Vec<_>>()
        })
        .await?;

        let show = move |editor: &mut Editor| {
            if let (Some(err), true) = (error, files.is_empty()) {
                editor.set_error(format!("Failed to list the changed files: {err}"));
                return;
            }
            let empty = files.is_empty();
            if !empty || editor.review.stream().is_some() {
                let sections = files.into_iter().map(|file| file.section(editor)).collect();
                review::show(editor, root, scope, sections);
                if let Some(path) = focus {
                    review::focus_file(editor, &path);
                }
            }
            match (empty, scope, done) {
                (true, ChangeView::All, _) => editor.set_status("There are no changes to review"),
                (true, scope, _) => {
                    editor.set_status(format!("There are no {} changes to review", scope.label()))
                }
                (false, _, Some(done)) => editor.set_status(done),
                (false, scope, None) => {
                    editor.set_status(format!("Reviewing {} changes", scope.label()))
                }
            }
        };
        Ok(Callback::Editor(Box::new(show)))
    });
}

/// The bytes of both sides of a changed file, read off the main thread.
struct File {
    change: FileChange,
    base: Vec<u8>,
    source: Vec<u8>,
    /// Whether the source is the index, not the file.
    staged: bool,
}

impl File {
    /// `scope`'s sides of `change`: `HEAD` or the index against the file, or
    /// `HEAD` against the index.
    fn read(
        providers: &DiffProviderRegistry,
        scope: ChangeView,
        change: FileChange,
        trust_full: bool,
    ) -> File {
        let base_path = match &change {
            FileChange::Renamed { from_path, .. } => from_path,
            _ => change.path(),
        };
        let head = |path: &Path| {
            providers
                .get_diff_base(path, trust_full)
                .unwrap_or_default()
        };
        let index = |path: &Path| {
            index::get_index_base(path, trust_full)
                .inspect_err(|err| log::debug!("failed to read the index version: {err:#}"))
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        let base = match (scope, &change) {
            // WHY: a conflicted file has no unconflicted index version.
            (ChangeView::Unstaged, FileChange::Conflict { .. }) => head(base_path),
            (ChangeView::Unstaged, _) => index(base_path),
            (ChangeView::All | ChangeView::Staged, _) => head(base_path),
        };
        let source = match (scope, &change) {
            (_, FileChange::Deleted { .. }) => Vec::new(),
            (ChangeView::Staged, _) => index(change.path()),
            _ => std::fs::read(change.path()).unwrap_or_default(),
        };
        File {
            change,
            base,
            source,
            staged: scope == ChangeView::Staged,
        }
    }

    fn section(self, editor: &Editor) -> Section {
        let doc = editor
            .document_by_path(self.change.path())
            .filter(|_| !self.staged);
        let (source, encoding) = match doc {
            Some(doc) => (doc.text().clone(), doc.encoding()),
            None => decode(&self.source, None),
        };
        let content =
            if crate::is_binary(&self.base) || (doc.is_none() && crate::is_binary(&self.source)) {
                Content::Binary
            } else {
                Content::Text {
                    base: decode(&self.base, Some(encoding)).0,
                    source,
                }
            };
        Section {
            change: self.change,
            content,
            encoding,
        }
    }
}

fn decode(
    bytes: &[u8],
    encoding: Option<&'static encoding::Encoding>,
) -> (Rope, &'static encoding::Encoding) {
    match from_reader(&mut &bytes[..], encoding) {
        Ok((text, encoding, _)) => (text, encoding),
        Err(_) => (Rope::new(), encoding.unwrap_or(encoding::UTF_8)),
    }
}

pub(super) fn open(cx: &mut Context) {
    let (view, doc) = current_ref!(cx.editor);
    let Some(stream) = cx.editor.review.stream_of(doc.id()) else {
        not_a_review(cx.editor);
        return;
    };
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let row = text.char_to_line(cursor);
    if let Some(Row::Gap { .. }) = stream.rows.get(row) {
        review::open_gap(cx.editor, row);
        return;
    }
    let Some(target) = stream.target(row) else {
        return;
    };
    let path = target.change.path().to_path_buf();
    if let FileChange::Deleted { .. } = target.change {
        let path = helix_stdx::path::get_relative_path(&path);
        cx.editor
            .set_error(format!("{} is deleted", path.display()));
        return;
    }
    let line = target.line;
    let column = target.same_line.then(|| cursor - text.line_to_char(row));

    let (view, doc) = current!(cx.editor);
    push_jump(view, doc);
    if let Err(err) = cx.editor.open(&path, Action::Replace) {
        cx.editor
            .set_error(format!("Failed to open {}: {err}", path.display()));
        return;
    }

    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let line = line.min(text.len_lines() - 1);
    let start = text.line_to_char(line);
    let pos = column.map_or(start, |column| {
        (start + column).min(line_end_char_index(&text, line))
    });
    doc.set_selection(view.id, Selection::point(pos));
    align_view(doc, view, Align::Center);
}

pub(super) fn next_hunk(cx: &mut Context) {
    step_hunk(cx, Direction::Forward);
}

pub(super) fn prev_hunk(cx: &mut Context) {
    step_hunk(cx, Direction::Backward);
}

fn step_hunk(cx: &mut Context, direction: Direction) {
    let count = cx.count() - 1;
    cx.editor.apply_motion(move |editor| {
        let Some(hunks) = stream_rows(editor, Stream::hunks) else {
            return;
        };
        let mode = editor.mode;
        let (view, doc) = current!(editor);
        let text = doc.text().slice(..);
        let selection = doc.selection(view.id).clone().transform(|range| {
            let row = range.cursor_line(text);
            // WHY: the rules of `]g` and `[g` in a file. The next hunk starts
            // below the cursor's row, and the previous one ends above it.
            let index = match direction {
                Direction::Forward => hunks
                    .iter()
                    .position(|hunk| hunk.start > row)
                    .map(|index| (index + count).min(hunks.len() - 1)),
                Direction::Backward => hunks
                    .iter()
                    .rposition(|hunk| hunk.end <= row)
                    .map(|index| index.saturating_sub(count)),
            };
            let Some(hunk) = index.map(|index| &hunks[index]) else {
                return range;
            };
            select_rows(text, range, hunk, mode, direction)
        });
        push_jump(view, doc);
        doc.set_selection(view.id, selection);
    });
}

pub(super) fn first_hunk(cx: &mut Context) {
    edge_hunk(cx, Direction::Backward);
}

pub(super) fn last_hunk(cx: &mut Context) {
    edge_hunk(cx, Direction::Forward);
}

fn edge_hunk(cx: &mut Context, end: Direction) {
    let Some(hunks) = stream_rows(cx.editor, Stream::hunks) else {
        return;
    };
    let hunk = match end {
        Direction::Forward => hunks.last(),
        Direction::Backward => hunks.first(),
    };
    let Some(hunk) = hunk else {
        return;
    };
    let mode = cx.editor.mode;
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let range = select_rows(
        text,
        doc.selection(view.id).primary(),
        hunk,
        mode,
        Direction::Forward,
    );
    push_jump(view, doc);
    doc.set_selection(view.id, Selection::single(range.anchor, range.head));
}

fn select_rows(
    text: RopeSlice,
    range: Range,
    rows: &std::ops::Range<usize>,
    mode: Mode,
    direction: Direction,
) -> Range {
    let rows = Range::new(text.line_to_char(rows.start), text.line_to_char(rows.end));
    if mode != Mode::Select {
        return rows.with_direction(direction);
    }
    let head = if rows.head < range.anchor {
        rows.anchor
    } else {
        rows.head
    };
    Range::new(range.anchor, head)
}

pub(super) fn next_file(cx: &mut Context) {
    step_file(cx, Direction::Forward);
}

pub(super) fn prev_file(cx: &mut Context) {
    step_file(cx, Direction::Backward);
}

fn step_file(cx: &mut Context, direction: Direction) {
    let count = cx.count() - 1;
    cx.editor.apply_motion(move |editor| {
        let Some(headers) = stream_rows(editor, |stream| stream.headers().collect::<Vec<_>>())
        else {
            return;
        };
        let extend = editor.mode == Mode::Select;
        let (view, doc) = current!(editor);
        let text = doc.text().slice(..);
        let selection = doc.selection(view.id).clone().transform(|range| {
            let row = range.cursor_line(text);
            let beyond: Vec<_> = match direction {
                Direction::Forward => headers.iter().filter(|&&header| header > row).collect(),
                Direction::Backward => headers
                    .iter()
                    .rev()
                    .filter(|&&header| header < row)
                    .collect(),
            };
            let Some(&header) = beyond.get(count).or(beyond.last()).copied() else {
                return range;
            };
            range.put_cursor(text, text.line_to_char(header), extend)
        });
        push_jump(view, doc);
        doc.set_selection(view.id, selection);
    });
}

pub(super) fn undo(cx: &mut Context) {
    if stream_rows(cx.editor, |_| ()).is_some() {
        review::undo(cx.editor, cx.count());
    }
}

pub(super) fn redo(cx: &mut Context) {
    if stream_rows(cx.editor, |_| ()).is_some() {
        review::redo(cx.editor, cx.count());
    }
}

fn stream_rows<T>(editor: &mut Editor, rows: impl Fn(&Stream) -> T) -> Option<T> {
    let rows = editor.review.stream_of(view!(editor).doc).map(rows);
    if rows.is_none() {
        not_a_review(editor);
    }
    rows
}

fn not_a_review(editor: &mut Editor) {
    editor.set_error("This buffer isn't a review. Start one with :review");
}
