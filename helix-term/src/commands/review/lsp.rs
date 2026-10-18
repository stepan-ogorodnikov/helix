//! The review's goto, references, hover, code actions, and rename. The first
//! ones run upstream's own command with the focused view lent to the file of
//! the row under the cursor. Their answers never read the buffer they asked
//! from: goto jumps from the current view, hover pops up at its cursor, and a
//! code action edits by path. The plan's 6.19 has the read of upstream behind
//! that. Rename has its own prompt.

use helix_core::syntax::config::LanguageServerFeature;
use helix_core::textobject::{textobject_word, TextObject};
use helix_lsp::block_on;
use helix_view::{doc, review};

use crate::commands::{
    code_action, goto_declaration, goto_definition, goto_implementation, goto_reference,
    goto_type_definition, hover, Context, MappableCommand,
};
use crate::ui::{self, PromptEvent};

/// The review's version of `command`, when the review lends its view for it.
/// Built here rather than in the command table: it exists only as a swap in
/// the review's keymap.
pub(crate) fn lent(command: &MappableCommand) -> Option<MappableCommand> {
    let MappableCommand::Static { name, doc, .. } = command else {
        return None;
    };
    let (name, fun): (_, fn(&mut Context)) = match *name {
        "goto_definition" => ("review_goto_definition", |cx| lend(cx, goto_definition)),
        "goto_declaration" => ("review_goto_declaration", |cx| lend(cx, goto_declaration)),
        "goto_type_definition" => ("review_goto_type_definition", |cx| {
            lend(cx, goto_type_definition)
        }),
        "goto_implementation" => ("review_goto_implementation", |cx| {
            lend(cx, goto_implementation)
        }),
        "goto_reference" => ("review_goto_reference", |cx| lend(cx, goto_reference)),
        "hover" => ("review_hover", |cx| lend(cx, hover)),
        "code_action" => ("review_code_action", |cx| lend(cx, code_action)),
        "rename_symbol" => ("review_rename_symbol", rename),
        _ => return None,
    };
    Some(MappableCommand::Static { name, fun, doc })
}

fn lend(cx: &mut Context, command: fn(&mut Context)) {
    let file = match review::lend(cx.editor) {
        Ok(file) => file,
        Err(refusal) => {
            cx.editor.set_error(refusal);
            return;
        }
    };
    let view = cx.editor.tree.focus;
    // INVARIANT: the view shows the review again before anything draws it or
    // a key reaches it. Upstream's command reads the buffer only before its
    // request goes out.
    let stream = std::mem::replace(&mut cx.editor.tree.get_mut(view).doc, file);
    command(cx);
    cx.editor.tree.get_mut(view).doc = stream;
}

// WHY: upstream's rename reads the focused buffer again when you confirm its
// prompt, and by then the view shows the review again. This one keeps the file
// and the place, and leaves out `prepareRename`.
fn rename(cx: &mut Context) {
    let file = match review::lend(cx.editor) {
        Ok(file) => file,
        Err(refusal) => {
            cx.editor.set_error(refusal);
            return;
        }
    };
    let view = cx.editor.tree.focus;
    let doc = doc!(cx.editor, &file);
    let Some(server) = doc
        .language_servers_with_feature(LanguageServerFeature::RenameSymbol)
        .next()
    else {
        cx.editor
            .set_error("No configured language server supports symbol renaming");
        return;
    };
    let (server, encoding) = (server.id(), server.offset_encoding());
    let position = doc.position(view, encoding);
    let version = doc.version();
    let text = doc.text().slice(..);
    let selection = doc.selection(view).primary();
    let word = match selection.len() > 1 {
        true => selection,
        false => textobject_word(text, selection, TextObject::Inside, 1, false),
    };
    let prefill = word.fragment(text).into_owned();

    let prompt = ui::Prompt::new(
        "rename-to:".into(),
        cx.register,
        ui::completers::none,
        move |cx, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate {
                return;
            }
            let Some(doc) = cx.editor.documents.get(&file) else {
                cx.editor
                    .set_error("The file closed, so nothing was renamed");
                return;
            };
            if doc.version() != version {
                cx.editor.set_error(format!(
                    "{} changed, so nothing was renamed",
                    doc.display_name()
                ));
                return;
            }
            let Some(server) = cx.editor.language_server_by_id(server) else {
                cx.editor.set_error("The language server exited");
                return;
            };
            let future = server
                .rename_symbol(doc.identifier(), position, input.to_string())
                .expect("the server supports renaming");
            let edit = match block_on(future) {
                Ok(edit) => edit.unwrap_or_default(),
                Err(err) => {
                    cx.editor.set_error(err.to_string());
                    return;
                }
            };
            // WHY: the rename is one step of the review's undo log, as an
            // edit is. A file it opens starts at revision 0.
            let mut before: Vec<_> = cx
                .editor
                .documents
                .iter_mut()
                .map(|(&id, doc)| (id, doc.get_current_revision()))
                .collect();
            let _ = cx.editor.apply_workspace_edit(encoding, &edit);
            let opened: Vec<_> = cx
                .editor
                .documents
                .keys()
                .filter(|id| !before.iter().any(|(known, _)| known == *id))
                .map(|&id| (id, 0))
                .collect();
            before.extend(opened);
            review::log_step(cx.editor, &before);
        },
    )
    .with_line(prefill, cx.editor);
    cx.push_layer(Box::new(prompt));
}
