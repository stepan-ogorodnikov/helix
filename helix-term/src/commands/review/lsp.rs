//! The review's goto, references, hover, and code actions. Each runs
//! upstream's own command with the focused view lent to the file of the row
//! under the cursor. Their answers never read the buffer they asked from: goto
//! jumps from the current view, hover pops up at its cursor, and a code action
//! edits by path. The plan's 6.19 has the read of upstream behind that.

use helix_view::review;

use crate::commands::{
    code_action, goto_declaration, goto_definition, goto_implementation, goto_reference,
    goto_type_definition, hover, Context, MappableCommand,
};

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
