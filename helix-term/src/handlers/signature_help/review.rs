//! Signature help in the review, from the language
//! server of the file under the cursor. The request lends the view to the
//! file's buffer, as goto does (`commands/review/lsp.rs`). The answer reads
//! only the focused buffer's language, for the popup's highlights. That's the
//! stream's again by then, so the signature shows without them.

use helix_core::syntax::config::LanguageServerFeature;
use helix_event::{send_blocking, TaskHandle};
use helix_lsp::lsp;
use helix_stdx::rope::RopeSliceExt;
use helix_view::handlers::lsp::{SignatureHelpEvent, SignatureHelpInvoked};
use helix_view::{current_ref, review, Editor};
use tokio::sync::mpsc::Sender;

use crate::job;

/// When the focused view shows the review, asks from the file under the
/// cursor, with the view lent to its buffer, and returns true.
pub(super) fn lend(
    editor: &mut Editor,
    invoked: SignatureHelpInvoked,
    cancel: &TaskHandle,
) -> bool {
    let view = editor.tree.focus;
    if editor.review.stream_of(editor.tree.get(view).doc).is_none() {
        return false;
    }
    match review::lend(editor) {
        Ok(file) => {
            // INVARIANT: the view shows the review again before anything
            // draws it. The request reads the buffer only before it goes out.
            let stream = std::mem::replace(&mut editor.tree.get_mut(view).doc, file);
            super::request_signature_help(editor, invoked, cancel.clone());
            editor.tree.get_mut(view).doc = stream;
        }
        Err(refusal) => {
            if invoked == SignatureHelpInvoked::Manual {
                editor.set_error(refusal);
            }
            // WHY: the handler waits for an answer, and a signature still
            // shown is from where the cursor was.
            tokio::spawn(job::dispatch(move |editor, compositor| {
                super::show_signature_help(editor, compositor, invoked, None)
            }));
        }
    }
    true
}

/// When the focused view shows the review, triggers signature help on the
/// trigger characters of the file's server, and returns true.
pub(super) fn trigger(editor: &Editor, tx: &Sender<SignatureHelpEvent>) -> bool {
    let (view, doc) = current_ref!(editor);
    if editor.review.stream_of(doc.id()).is_none() {
        return false;
    }
    let Some(ls) = review::file_at_cursor(editor).and_then(|file| {
        file.language_servers_with_feature(LanguageServerFeature::SignatureHelp)
            .next()
    }) else {
        return true;
    };
    if let Some(lsp::SignatureHelpOptions {
        trigger_characters: Some(triggers),
        ..
    }) = &ls.capabilities().signature_help_provider
    {
        let text = doc.text().slice(..);
        let before = text.slice(..doc.selection(view.id).primary().cursor(text));
        if triggers.iter().any(|trigger| before.ends_with(trigger)) {
            send_blocking(tx, SignatureHelpEvent::Trigger)
        }
    }
    true
}
