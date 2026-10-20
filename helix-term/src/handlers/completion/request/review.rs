//! FORK: review-stream. Completion in the review, from the language servers
//! of the file under the cursor. The stream has none of its own.
//!
//! The requests go out from the file's buffer, with the stream's savepoint.
//! The items keep the file's places: the menu moves one onto the stream's rows
//! as it accepts it (`review::accept_item`), since a resolve can answer with
//! the file's places again. The sync then forwards it to the file.

use std::collections::HashSet;
use std::sync::Arc;

use helix_core::completion::CompletionProvider;
use helix_core::syntax::config::LanguageServerFeature;
use helix_core::RopeSlice;
use helix_event::TaskHandle;
use helix_lsp::lsp;
use helix_stdx::rope::RopeSliceExt;
use helix_view::document::SavePoint;
use helix_view::{current, review, view, Editor};
use tokio::task::JoinSet;

use super::{request_completions_from_language_server, TriggerKind};
use crate::handlers::completion::{replace_completions, CompletionResponse};

fn in_review(editor: &Editor) -> bool {
    editor.review.stream_of(view!(editor).doc).is_some()
}

/// Asks the servers of the file under the cursor, when the focused view
/// shows the review.
pub(in crate::handlers::completion) fn request(
    editor: &mut Editor,
    kind: TriggerKind,
    savepoint: &Arc<SavePoint>,
    requests: &mut JoinSet<CompletionResponse>,
) {
    if !in_review(editor) {
        return;
    }
    let Ok(file) = review::lend(editor) else {
        return;
    };
    let view = editor.tree.focus;
    let doc = &editor.documents[&file];
    let text = doc.text().slice(..);
    let before = text.slice(..doc.selection(view).primary().cursor(text));
    let mut seen = HashSet::new();
    let servers = doc
        .language_servers_with_feature(LanguageServerFeature::Completion)
        .filter(|ls| seen.insert(ls.id()));
    for (priority, ls) in servers.enumerate() {
        requests.spawn(request_completions_from_language_server(
            ls,
            doc,
            view,
            context(ls, kind, before),
            -(priority as i8),
            savepoint.clone(),
        ));
    }
}

/// Asks the servers whose lists were incomplete again, when the focused view
/// shows the review. Returns whether it does.
pub(in crate::handlers::completion) fn request_incomplete(
    editor: &mut Editor,
    handle: &TaskHandle,
) -> bool {
    if !in_review(editor) {
        return false;
    }
    let incomplete: Vec<_> = editor
        .handlers
        .completions
        .active_completions
        .iter()
        .filter(|(_, context)| context.is_incomplete)
        .filter_map(|(provider, context)| match provider {
            CompletionProvider::Lsp(id) => Some((*id, context.priority)),
            _ => None,
        })
        .collect();
    if incomplete.is_empty() {
        return true;
    }
    // WHY: this runs inside the key that typed the char, before the frame's
    // sync forwards it. The menu is open, so the rows stay as they are.
    review::sync(editor, true);
    let Ok(file) = review::lend(editor) else {
        return true;
    };
    let (view, stream) = current!(editor);
    let savepoint = stream.savepoint(view);
    let view = view.id;
    let doc = &editor.documents[&file];
    let mut requests = JoinSet::new();
    for (id, priority) in incomplete {
        let Some(ls) = editor.language_servers.get_by_id(id) else {
            continue;
        };
        let context = lsp::CompletionContext {
            trigger_kind: lsp::CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS,
            trigger_character: None,
        };
        requests.spawn(request_completions_from_language_server(
            ls,
            doc,
            view,
            context,
            priority,
            savepoint.clone(),
        ));
    }
    if !requests.is_empty() {
        tokio::spawn(replace_completions(handle.clone(), requests, true));
    }
    true
}

/// Whether `before`, the text before the cursor, ends with a trigger
/// character of a server of the file under the cursor.
pub(in crate::handlers::completion) fn is_trigger(editor: &Editor, before: RopeSlice) -> bool {
    let Some(file) = review::file_at_cursor(editor) else {
        return false;
    };
    file.language_servers_with_feature(LanguageServerFeature::Completion)
        .any(|ls| {
            matches!(&ls.capabilities().completion_provider, Some(lsp::CompletionOptions {
                trigger_characters: Some(triggers),
                ..
            }) if triggers.iter().any(|trigger| before.ends_with(trigger)))
        })
}

// WHY: a copy of the context `request_completions` builds inline.
fn context(ls: &helix_lsp::Client, kind: TriggerKind, before: RopeSlice) -> lsp::CompletionContext {
    let trigger_character = match kind {
        TriggerKind::Manual => None,
        _ => ls
            .capabilities()
            .completion_provider
            .as_ref()
            .and_then(|provider| {
                provider
                    .trigger_characters
                    .as_deref()?
                    .iter()
                    .find(|&trigger| before.ends_with(trigger))
            })
            .cloned(),
    };
    let trigger_kind = match trigger_character {
        Some(_) => lsp::CompletionTriggerKind::TRIGGER_CHARACTER,
        None => lsp::CompletionTriggerKind::INVOKED,
    };
    lsp::CompletionContext {
        trigger_kind,
        trigger_character,
    }
}
