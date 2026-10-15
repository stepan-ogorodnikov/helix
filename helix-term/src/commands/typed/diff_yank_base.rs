//! FORK: inline diff. `:diff-yank-base` copies the base text of changes, since
//! the cursor can't enter the removed rows.

use helix_core::command_line::Args;
use helix_core::Rope;
use helix_vcs::Hunk;
use helix_view::current_ref;

use crate::compositor;
use crate::ui::PromptEvent;

/// The base text of `hunks`, joined in order, and how many hunks there were.
fn base_text<'a>(base: &Rope, hunks: impl Iterator<Item = &'a Hunk>) -> (usize, String) {
    let mut text = String::new();
    let mut count = 0;
    for hunk in hunks {
        count += 1;
        let start = base.line_to_char(hunk.before.start as usize);
        let end = base.line_to_char(hunk.before.end as usize);
        text.extend(base.slice(start..end).chunks());
    }
    (count, text)
}

pub(super) fn diff_yank_base(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let reg = if args.is_empty() {
        cx.editor
            .selected_register
            .unwrap_or(cx.editor.config().default_yank_register)
    } else {
        super::yank_register(&args)?
    };
    let (view, doc) = current_ref!(cx.editor);
    let Some(handle) = doc.diff_handle() else {
        anyhow::bail!("Diff is not available in the current buffer");
    };
    let text = doc.text().slice(..);
    let diff = handle.load();
    let (count, base) = base_text(
        diff.diff_base(),
        diff.hunks_intersecting_line_ranges(doc.selection(view.id).line_ranges(text)),
    );
    drop(diff);
    if count == 0 {
        anyhow::bail!("There are no changes under any selection");
    }
    if base.is_empty() {
        anyhow::bail!("The changes under the selections only add lines");
    }

    cx.editor.registers.write(reg, vec![base])?;
    cx.editor.set_status(format!(
        "Yanked the base text of {count} change{} to register {reg}",
        if count == 1 { "" } else { "s" }
    ));
    Ok(())
}

#[cfg(test)]
mod test {
    use helix_core::Rope;
    use helix_vcs::Hunk;

    use super::base_text;

    #[test]
    fn joins_the_base_lines_of_each_hunk() {
        let base = Rope::from("a\nold1\nold2\nb\ngone\n");
        let hunks = [
            Hunk {
                before: 1..3,
                after: 1..2,
            },
            // A pure insertion adds nothing.
            Hunk {
                before: 4..4,
                after: 3..4,
            },
            Hunk {
                before: 4..5,
                after: 5..5,
            },
        ];
        assert_eq!(
            base_text(&base, hunks.iter()),
            (3, "old1\nold2\ngone\n".to_string())
        );
    }
}
