//! `:review` and the commands of the review stream. The rows and the buffer
//! live in `helix_view::review`. This module lists the changed files, reads
//! both sides of each, and opens a row's file.

use std::path::Path;

use helix_core::command_line::Args;
use helix_core::line_ending::line_end_char_index;
use helix_core::{encoding, Rope, Selection};
use helix_loader::workspace_trust::TrustQuery;
use helix_vcs::{visible_changes, ChangeKind, ChangeView, DiffProviderRegistry, FileChange};
use helix_view::document::from_reader;
use helix_view::editor::Action;
use helix_view::review::{self, Content, Section};
use helix_view::{align_view, current, current_ref, doc, Align, Editor};

use super::{push_jump, Context};
use crate::compositor;
use crate::job::Callback;
use crate::ui::PromptEvent;

pub(super) fn review(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

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
    let providers = cx.editor.diff_providers.clone();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    providers
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
        let mut changes = visible_changes(ChangeView::All, &unstaged, &staged);
        changes.sort_by(|a, b| a.path().cmp(b.path()));
        let files = tokio::task::spawn_blocking(move || {
            changes
                .into_iter()
                .map(|change| File::read(&providers, change, trust_full))
                .collect::<Vec<_>>()
        })
        .await?;

        let show = move |editor: &mut Editor| match error {
            Some(err) if files.is_empty() => {
                editor.set_error(format!("Failed to list the changed files: {err}"))
            }
            _ if files.is_empty() => editor.set_status("There are no changes to review"),
            _ => {
                let sections = files.into_iter().map(|file| file.section(editor)).collect();
                review::show(editor, sections);
            }
        };
        Ok(Callback::Editor(Box::new(show)))
    });
    Ok(())
}

/// The bytes of both sides of a changed file, read off the main thread.
struct File {
    change: FileChange,
    base: Vec<u8>,
    source: Vec<u8>,
}

impl File {
    fn read(providers: &DiffProviderRegistry, change: FileChange, trust_full: bool) -> File {
        let base_path = match &change {
            FileChange::Renamed { from_path, .. } => from_path,
            _ => change.path(),
        };
        let base = providers
            .get_diff_base(base_path, trust_full)
            .unwrap_or_default();
        let source = match change {
            FileChange::Deleted { .. } => Vec::new(),
            _ => std::fs::read(change.path()).unwrap_or_default(),
        };
        File {
            change,
            base,
            source,
        }
    }

    fn section(self, editor: &Editor) -> Section {
        let doc = editor.document_by_path(self.change.path());
        let content =
            if crate::is_binary(&self.base) || (doc.is_none() && crate::is_binary(&self.source)) {
                Content::Binary
            } else {
                let (source, encoding) = match doc {
                    Some(doc) => (doc.text().clone(), doc.encoding()),
                    None => decode(&self.source, None),
                };
                Content::Text {
                    base: decode(&self.base, Some(encoding)).0,
                    source,
                }
            };
        Section {
            change: self.change,
            content,
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
        cx.editor
            .set_error("This buffer isn't a review. Start one with :review");
        return;
    };
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let row = text.char_to_line(cursor);
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
