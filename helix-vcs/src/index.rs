//! The index as a diff base, and staging, for the review stream. Builds without
//! the `git` feature get an error, like the diff providers.

#[cfg(feature = "git")]
pub use crate::git::index::{get_index_base, set_index_text};

#[cfg(not(feature = "git"))]
pub fn get_index_base(
    _file: &std::path::Path,
    _trust_full: bool,
) -> anyhow::Result<Option<Vec<u8>>> {
    anyhow::bail!("No diff support compiled in")
}

#[cfg(not(feature = "git"))]
pub fn set_index_text(
    _file: &std::path::Path,
    _text: Option<&[u8]>,
    _trust_full: bool,
) -> anyhow::Result<()> {
    anyhow::bail!("No diff support compiled in")
}
