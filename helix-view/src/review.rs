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
}
