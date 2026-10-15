//! FORK: diff review. Phase 1 is the per-view inline diff toggle.

use helix_view::current;

use super::Context;

pub(super) fn toggle_inline_diff(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    view.inline_diff.enabled = !view.inline_diff.enabled;
    if view.inline_diff.enabled && doc.diff_handle().is_none() {
        cx.editor
            .set_status("Inline diff is on, but this file has no diff base");
    }
}
