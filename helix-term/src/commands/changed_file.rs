use std::collections::HashSet;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use helix_event::status;
use helix_vcs::{visible_changes, ChangeKind, ChangeView, FileChange};
use helix_view::theme::Style;
use tui::text::Span;

use crate::job;
use crate::ui::overlay::{overlaid, Overlay};
use crate::ui::{Picker, PickerColumn};

use super::Context;

pub(super) fn changed_file_picker(cx: &mut Context) {
    pub struct FileChangeData {
        cwd: PathBuf,
        style_untracked: Style,
        style_modified: Style,
        style_conflict: Style,
        style_deleted: Style,
        style_renamed: Style,
    }

    let cwd = helix_stdx::env::current_working_dir();
    if !cwd.exists() {
        cx.editor
            .set_error("Current working directory does not exist");
        return;
    }

    let added = cx.editor.theme.get("diff.plus");
    let modified = cx.editor.theme.get("diff.delta");
    let conflict = cx.editor.theme.get("diff.delta.conflict");
    let deleted = cx.editor.theme.get("diff.minus");
    let renamed = cx.editor.theme.get("diff.delta.moved");

    let columns = [
        PickerColumn::new("change", |change: &FileChange, data: &FileChangeData| {
            match change {
                FileChange::Untracked { .. } => Span::styled("+ untracked", data.style_untracked),
                FileChange::Added { .. } => Span::styled("+ added", data.style_untracked),
                FileChange::Modified { .. } => Span::styled("~ modified", data.style_modified),
                FileChange::Conflict { .. } => Span::styled("x conflict", data.style_conflict),
                FileChange::Deleted { .. } => Span::styled("- deleted", data.style_deleted),
                FileChange::Renamed { .. } => Span::styled("> renamed", data.style_renamed),
            }
            .into()
        }),
        PickerColumn::new("path", |change: &FileChange, data: &FileChangeData| {
            let display_path = |path: &PathBuf| {
                path.strip_prefix(&data.cwd)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            };
            match change {
                FileChange::Untracked { path } => display_path(path),
                FileChange::Added { path } => display_path(path),
                FileChange::Modified { path } => display_path(path),
                FileChange::Conflict { path } => display_path(path),
                FileChange::Deleted { path } => display_path(path),
                FileChange::Renamed { from_path, to_path } => {
                    format!("{} -> {}", display_path(from_path), display_path(to_path))
                }
            }
            .into()
        }),
    ];

    struct Lists {
        unstaged: Vec<FileChange>,
        staged: Vec<FileChange>,
        mode: ChangeView,
        /// Paths already shown for the current mode. `All` uses this so a path that is
        /// both staged and unstaged is injected once.
        shown: HashSet<PathBuf>,
        /// Present while the status walk is still running. Dropped when the walk ends so
        /// the match count stops saying `(running)`.
        injector: Option<crate::ui::picker::Injector<FileChange, FileChangeData>>,
    }

    impl Lists {
        fn record(&mut self, kind: ChangeKind, change: FileChange) -> bool {
            let alive = self
                .injector
                .as_ref()
                .is_some_and(|injector| injector.is_alive());
            if !alive {
                return false;
            }
            let path = change.path().to_path_buf();
            let inject = self.mode.includes(kind) && self.shown.insert(path);
            if inject
                && self
                    .injector
                    .as_ref()
                    .unwrap()
                    .push(change.clone())
                    .is_err()
            {
                return false;
            }
            match kind {
                ChangeKind::Unstaged => self.unstaged.push(change),
                ChangeKind::Staged => self.staged.push(change),
            }
            true
        }
    }

    /// Clears the live injector when the status walk drops its callback, and
    /// rebuilds the `All` list if it's showing.
    struct FinishScan(Arc<Mutex<Lists>>);

    impl Drop for FinishScan {
        fn drop(&mut self) {
            let Some(injector) = self.0.lock().ok().and_then(|mut lists| {
                let injector = lists.injector.take();
                injector.filter(|_| lists.mode == ChangeView::All)
            }) else {
                return;
            };
            // WHY: the live `All` list took each path's first entry as the walk
            // found it, so a staged rename that came after the path's unstaged
            // modification said modified. Rebuilt, it says what each file is
            // against `HEAD`, in path order.
            let lists = Arc::clone(&self.0);
            job::dispatch_blocking(move |_, compositor| {
                let mut lists = lists.lock().unwrap();
                if !injector.is_alive() || lists.mode != ChangeView::All {
                    return;
                }
                let Some(Overlay {
                    content: picker, ..
                }) = compositor.find::<Overlay<Picker<FileChange, FileChangeData>>>()
                else {
                    return;
                };
                let items = visible_changes(ChangeView::All, &lists.unstaged, &lists.staged);
                lists.shown = items
                    .iter()
                    .map(|change| change.path().to_path_buf())
                    .collect();
                picker.replace_items(items);
            });
        }
    }

    let picker = Picker::new(
        columns,
        1, // path
        [],
        FileChangeData {
            cwd: cwd.clone(),
            style_untracked: added,
            style_modified: modified,
            style_conflict: conflict,
            style_deleted: deleted,
            style_renamed: renamed,
        },
        |cx, meta: &FileChange, action| {
            let path_to_open = meta.path();
            if let Err(e) = cx.editor.open(path_to_open, action) {
                let err = if let Some(err) = e.source() {
                    format!("{}", err)
                } else {
                    format!("unable to open \"{}\"", path_to_open.display())
                };
                cx.editor.set_error(err);
            }
        },
    )
    .with_preview(|_editor, meta| Some((meta.path().into(), None)))
    .with_status_label(ChangeView::Unstaged.label());

    let lists = Arc::new(Mutex::new(Lists {
        unstaged: Vec::new(),
        staged: Vec::new(),
        mode: ChangeView::Unstaged,
        shown: HashSet::new(),
        injector: Some(picker.injector()),
    }));
    let lists_for_cycle = Arc::clone(&lists);
    let lists_for_review = Arc::clone(&lists);
    let lists_for_scan = Arc::clone(&lists);
    let finish_scan = FinishScan(lists);
    let picker = picker.with_on_cycle_changes(move |picker| {
        let mut lists = lists_for_cycle.lock().unwrap();
        lists.mode = lists.mode.cycle();
        let items = visible_changes(lists.mode, &lists.unstaged, &lists.staged);
        lists.shown = items
            .iter()
            .map(|change| change.path().to_path_buf())
            .collect();
        let injector = picker.replace_items(items);
        if lists.injector.is_some() {
            lists.injector = Some(injector);
        }
        picker.set_status_label(lists.mode.label());
    });
    // FORK: review-stream
    let picker = picker.with_on_review_jump(move |cx, change: &FileChange| {
        let scope = lists_for_review.lock().unwrap().mode;
        super::review::jump(cx, scope, change.path());
    });

    let trust_full = cx
        .editor
        .workspace_trust
        .query(
            &helix_loader::find_workspace_in(&cwd).0,
            helix_loader::workspace_trust::TrustQuery::Git,
        )
        .is_trusted();
    cx.editor
        .diff_providers
        .clone()
        .for_each_changed_file(cwd, trust_full, move |change| {
            let _finish_scan = &finish_scan;
            match change {
                Ok((kind, change)) => lists_for_scan.lock().unwrap().record(kind, change),
                Err(err) => {
                    status::report_blocking(err);
                    true
                }
            }
        });
    cx.push_layer(Box::new(overlaid(picker)));
}
