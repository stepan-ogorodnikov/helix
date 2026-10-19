use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Which side of `git status` a [`FileChange`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Difference between the index and the working tree, including untracked files.
    Unstaged,
    /// Difference between `HEAD` and the index.
    Staged,
}

/// Which list the changed-file picker is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeView {
    Unstaged,
    Staged,
    All,
}

impl ChangeView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
            Self::All => "all",
        }
    }

    /// `unstaged`, then `staged`, then `all`.
    pub fn cycle(self) -> Self {
        match self {
            Self::Unstaged => Self::Staged,
            Self::Staged => Self::All,
            Self::All => Self::Unstaged,
        }
    }

    pub fn includes(self, kind: ChangeKind) -> bool {
        match self {
            Self::Unstaged => kind == ChangeKind::Unstaged,
            Self::Staged => kind == ChangeKind::Staged,
            Self::All => true,
        }
    }
}

/// States for a file having been changed.
#[derive(Clone)]
pub enum FileChange {
    /// Not tracked by the VCS.
    Untracked { path: PathBuf },
    /// Staged addition, or a staged copy.
    Added { path: PathBuf },
    /// File has been modified.
    Modified { path: PathBuf },
    /// File modification is in conflict with a different update.
    Conflict { path: PathBuf },
    /// File has been deleted.
    Deleted { path: PathBuf },
    /// File has been renamed.
    Renamed {
        from_path: PathBuf,
        to_path: PathBuf,
    },
}

impl FileChange {
    pub fn path(&self) -> &Path {
        match self {
            Self::Untracked { path } => path,
            Self::Added { path } => path,
            Self::Modified { path } => path,
            Self::Conflict { path } => path,
            Self::Deleted { path } => path,
            Self::Renamed { to_path, .. } => to_path,
        }
    }
}

/// Files for `view`, in path order. `All` lists each path once, as it is
/// against `HEAD`: a path both staged and unstaged keeps its unstaged entry,
/// unless that's a modification, which leaves the file what its staged change
/// made it, such as added or renamed.
pub fn visible_changes(
    view: ChangeView,
    unstaged: &[FileChange],
    staged: &[FileChange],
) -> Vec<FileChange> {
    let mut changes: Vec<FileChange> = match view {
        ChangeView::Unstaged => unstaged.to_vec(),
        ChangeView::Staged => staged.to_vec(),
        ChangeView::All => {
            let staged_at: HashMap<&Path, &FileChange> = staged
                .iter()
                .map(|change| (change.path(), change))
                .collect();
            let mut seen = HashSet::new();
            unstaged
                .iter()
                .map(|change| match (change, staged_at.get(change.path())) {
                    (FileChange::Modified { .. }, Some(staged)) => staged,
                    _ => change,
                })
                .chain(staged.iter())
                .filter(|change| seen.insert(change.path().to_path_buf()))
                .cloned()
                .collect()
        }
    };
    changes.sort_by(|a, b| a.path().cmp(b.path()));
    changes
}

#[cfg(test)]
mod test {
    use super::*;

    fn label(change: &FileChange) -> String {
        match change {
            FileChange::Untracked { .. } => "untracked".into(),
            FileChange::Added { .. } => "added".into(),
            FileChange::Modified { .. } => "modified".into(),
            FileChange::Conflict { .. } => "conflict".into(),
            FileChange::Deleted { .. } => "deleted".into(),
            FileChange::Renamed { from_path, .. } => {
                format!("renamed from {}", from_path.display())
            }
        }
    }

    #[test]
    fn all_lists_a_staged_and_unstaged_path_once_as_it_is_against_head() {
        let path = PathBuf::from("a.txt");
        let modified = FileChange::Modified { path: path.clone() };
        let cases = [
            (
                "added, then modified",
                FileChange::Added { path: path.clone() },
                modified.clone(),
                "added",
            ),
            (
                "renamed, then modified",
                FileChange::Renamed {
                    from_path: PathBuf::from("old.txt"),
                    to_path: path.clone(),
                },
                modified.clone(),
                "renamed from old.txt",
            ),
            (
                "modified, then deleted",
                modified.clone(),
                FileChange::Deleted { path: path.clone() },
                "deleted",
            ),
        ];
        for (case, staged, unstaged, expected) in cases {
            let all = visible_changes(ChangeView::All, &[unstaged], &[staged]);
            assert_eq!(
                all.iter().map(label).collect::<Vec<_>>(),
                [expected],
                "{case}"
            );
        }
    }

    #[test]
    fn each_list_is_in_path_order() {
        let modified = |name: &str| FileChange::Modified {
            path: PathBuf::from(name),
        };
        let unstaged = [modified("c"), modified("a")];
        let staged = [modified("d"), modified("b")];
        let paths = |view| {
            visible_changes(view, &unstaged, &staged)
                .iter()
                .map(|change| change.path().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(ChangeView::Unstaged), ["a", "c"]);
        assert_eq!(paths(ChangeView::Staged), ["b", "d"]);
        assert_eq!(
            paths(ChangeView::All),
            ["a", "b", "c", "d"],
            "staged-only files among the rest"
        );
    }
}
