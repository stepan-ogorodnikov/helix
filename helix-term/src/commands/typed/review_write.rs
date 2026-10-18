//! FORK: review-stream. `:w` in the review writes every file it shows with
//! unsaved changes, each through upstream's chain: trims, final newline, code
//! actions and format on save, then the save. The save's own edits are steps of
//! the review's undo log, so `u` undoes a format first, as in a file.

use std::future::Future;
use std::path::PathBuf;

use anyhow::bail;
use helix_vcs::ChangeView;
use helix_view::{doc, doc_mut, review, view_mut, DocumentId, Editor};

use super::{
    code_actions_on_save, insert_final_newline, make_format_callback, trim_final_newlines,
    trim_trailing_whitespace, WriteOptions,
};
use crate::compositor;
use crate::job::{self, Callback, Job};

/// Writes the review's files when the current buffer is the review, or `None`
/// when it isn't.
pub(super) fn write(
    cx: &mut compositor::Context,
    path: Option<&str>,
    options: WriteOptions,
) -> Option<anyhow::Result<()>> {
    let doc = doc!(cx.editor).id();
    cx.editor.review.stream_of(doc)?;
    Some(write_files(cx, path, options))
}

// WHY: a copy of `write_all_impl`'s chain, which writes every modified buffer
// and takes no filter. Keep it in step with upstream's.
fn write_files(
    cx: &mut compositor::Context,
    path: Option<&str>,
    options: WriteOptions,
) -> anyhow::Result<()> {
    let stream = cx.editor.review.stream().expect("checked by the caller");
    if stream.scope == ChangeView::Staged {
        bail!("A staged review is read-only. ret opens the file.");
    }
    if path.is_some() {
        bail!("The review writes each file to its own path");
    }
    let docs: Vec<DocumentId> = stream
        .paths()
        .filter_map(|path| cx.editor.document_by_path(path))
        .filter(|doc| doc.is_modified())
        .map(|doc| doc.id())
        .collect();
    if docs.is_empty() {
        cx.editor
            .set_status("No file in the review has unsaved changes");
        return Ok(());
    }

    let config = cx.editor.config();
    let mut trimmed = Vec::with_capacity(docs.len());
    let mut saves = Vec::with_capacity(docs.len());
    for doc_id in docs {
        let target_view = cx.editor.get_synced_view_id(doc_id);
        let doc = doc_mut!(cx.editor, &doc_id);
        let view = view_mut!(cx.editor, target_view);
        trimmed.push((doc_id, doc.get_current_revision()));

        if doc.trim_trailing_whitespace() {
            trim_trailing_whitespace(doc, target_view);
        }
        if config.trim_final_newlines {
            trim_final_newlines(doc, target_view);
        }
        if doc.insert_final_newline() {
            insert_final_newline(doc, target_view);
        }

        // Save an undo checkpoint for any outstanding changes.
        doc.append_changes_to_history(view);
        saves.push((doc_id, target_view));
    }
    // INVARIANT: before any save, which `?` can cut short. The trims of all the
    // files are one step.
    review::log_step(cx.editor, &trimmed);

    for (doc_id, target_view) in saves {
        let auto_format = config.auto_format && options.auto_format;
        let force = options.force;

        let run_code_actions = options.code_actions
            && doc!(cx.editor, &doc_id)
                .language_config()
                .and_then(|c| c.code_actions_on_save.as_deref())
                .is_some_and(|kinds| !kinds.is_empty());

        let tail = (auto_format || run_code_actions).then(|| {
            let callback: job::Callback = Callback::Followup(Box::new(move |editor| {
                // The document could have been closed mid-chain
                if !editor.documents.contains_key(&doc_id) {
                    return None;
                }
                let doc = doc!(editor, &doc_id);
                let fmt_job = auto_format
                    .then(|| doc.auto_format(editor))
                    .flatten()
                    .map(|fmt| {
                        let call = make_format_callback(
                            doc_id,
                            doc.version(),
                            target_view,
                            fmt,
                            Some((None, force)),
                        );
                        Job::with_callback(logged(doc_id, call)).wait_before_exiting()
                    });
                if fmt_job.is_none() {
                    if let Err(err) = editor.save::<PathBuf>(doc_id, None, force) {
                        editor.set_error(format!("Error saving: {}", err));
                    }
                }
                fmt_job
            }));
            Job::with_callback(async { Ok(callback) }).wait_before_exiting()
        });

        let job = if run_code_actions {
            code_actions_on_save(cx, doc_id, tail)
        } else {
            tail
        };

        if let Some(job) = job {
            cx.jobs.add(job);
        } else {
            cx.editor.save::<PathBuf>(doc_id, None, force)?;
        }
    }
    Ok(())
}

/// Logs the format that `call` applies to `doc` as a step of the review's undo
/// log, when it lands.
async fn logged(
    doc: DocumentId,
    call: impl Future<Output = anyhow::Result<job::Callback>>,
) -> anyhow::Result<job::Callback> {
    let Callback::Editor(apply) = call.await? else {
        unreachable!("a format callback edits the editor");
    };
    Ok(Callback::Editor(Box::new(move |editor: &mut Editor| {
        let before = editor
            .documents
            .get_mut(&doc)
            .map(|doc| doc.get_current_revision());
        apply(editor);
        if let Some(before) = before {
            review::log_step(editor, &[(doc, before)]);
        }
    })))
}
