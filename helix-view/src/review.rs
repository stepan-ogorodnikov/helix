//! FORK: diff review. The changed files a view steps through.

use helix_vcs::FileChange;

use crate::DocumentId;

/// A review session. It belongs to the view that started it.
#[derive(Clone)]
pub struct ReviewSession {
    /// Changed files in review order. Never empty.
    pub files: Vec<FileChange>,
    /// The file the review opened last, as an index into `files`.
    pub current: usize,
    /// The document of `files[current]`. `None` when it failed to open.
    pub doc: Option<DocumentId>,
    /// What each file diffs against.
    pub base: ReviewBase,
}

/// What a review diffs each file against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewBase {
    /// The last commit, so the review shows every change.
    Head,
    /// The index, so the review shows only what isn't staged.
    Index,
}
