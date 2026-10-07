use std::collections::HashSet;
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

/// Files for `view`. `All` lists each path once and keeps the unstaged entry when a
/// path is both staged and unstaged.
pub fn visible_changes(
    view: ChangeView,
    unstaged: &[FileChange],
    staged: &[FileChange],
) -> Vec<FileChange> {
    match view {
        ChangeView::Unstaged => unstaged.to_vec(),
        ChangeView::Staged => staged.to_vec(),
        ChangeView::All => {
            let mut seen = HashSet::new();
            unstaged
                .iter()
                .chain(staged.iter())
                .filter(|change| seen.insert(change.path().to_path_buf()))
                .cloned()
                .collect()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn all_keeps_one_entry_and_prefers_unstaged() {
        let path = PathBuf::from("a.txt");
        let unstaged = vec![FileChange::Modified { path: path.clone() }];
        let staged = vec![FileChange::Added { path: path.clone() }];

        let all = visible_changes(ChangeView::All, &unstaged, &staged);
        assert!(matches!(all.as_slice(), [FileChange::Modified { .. }]));
        assert!(matches!(
            visible_changes(ChangeView::Staged, &unstaged, &staged).as_slice(),
            [FileChange::Added { .. }]
        ));
        assert_eq!(
            visible_changes(ChangeView::Unstaged, &unstaged, &staged).len(),
            1
        );
    }
}
