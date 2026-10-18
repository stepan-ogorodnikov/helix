//! `:review-restore` puts the base's text back over the hunks the selection
//! touches in the review, in each file's buffer, as one step of the review's
//! undo log. The review follows the buffer like any other edit.

use anyhow::{bail, Context as _};
use helix_core::command_line::Args;
use helix_core::Transaction;
use helix_vcs::ChangeView;
use helix_view::review::{self, layout};
use helix_view::{current_ref, Editor};

use super::open_stream;
use super::stage::under;
use crate::compositor;
use crate::ui::PromptEvent;

pub(in crate::commands) fn review_restore(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (view, doc) = current_ref!(cx.editor);
    let stream = open_stream(cx.editor)
        .filter(|stream| stream.doc == doc.id())
        .context("This buffer isn't a review. Start one with :review")?;
    if stream.scope == ChangeView::Staged {
        bail!("A staged review shows the index, not a file. :unstage-hunk takes a hunk back out");
    }
    let rows: Vec<_> = doc
        .selection(view.id)
        .line_ranges(doc.text().slice(..))
        .collect();
    let picked: Vec<_> = stream
        .picked(&rows)
        .into_iter()
        .map(|file| {
            (
                file.change.path().to_path_buf(),
                file.base.clone(),
                file.lines,
            )
        })
        .collect();

    let mut restored = 0;
    for (path, base, lines) in picked {
        restored += restore(cx.editor, &path, &base, &lines).with_context(|| {
            let path = helix_stdx::path::get_relative_path(&path);
            format!("{}", path.display())
        })?;
    }
    match restored {
        0 => bail!("There are no changes under any selection"),
        1 => cx.editor.set_status("Restored 1 change"),
        count => cx.editor.set_status(format!("Restored {count} changes")),
    }
    Ok(())
}

/// Puts `base`'s lines back over the hunks of `path`'s buffer under `lines`.
/// Opens the buffer in the background when it isn't open. Returns how many.
fn restore(
    editor: &mut Editor,
    path: &std::path::Path,
    base: &helix_core::Rope,
    lines: &[(usize, usize)],
) -> anyhow::Result<usize> {
    let doc_id = match editor.document_by_path(path) {
        Some(doc) => doc.id(),
        None => review::load(editor, path)?,
    };
    let text = editor.documents[&doc_id].text().clone();
    let hunks = layout::hunks(base, &text).context("The file is too large to diff")?;
    let picked: Vec<_> = hunks
        .iter()
        .filter(|hunk| under(&hunk.after, lines))
        .collect();
    if picked.is_empty() {
        return Ok(0);
    }
    let changes = picked.iter().map(|hunk| {
        let base_lines = base.line_to_char(hunk.before.start as usize)
            ..base.line_to_char(hunk.before.end as usize);
        (
            text.line_to_char(hunk.after.start as usize),
            text.line_to_char(hunk.after.end as usize),
            Some(base.slice(base_lines).to_string().into()),
        )
    });
    let transaction = Transaction::change(&text, changes);
    review::edit_file(editor, doc_id, editor.tree.focus, &transaction);
    Ok(picked.len())
}
