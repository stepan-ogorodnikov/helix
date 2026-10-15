//! FORK: diff review. `:stage-hunk` and `:unstage-hunk` write the changes under
//! the selections to the index.

use std::ops::Range;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context as _};
use helix_core::command_line::Args;
use helix_core::encoding::Encoding;
use helix_core::{Rope, RopeSlice};
use helix_vcs::{index, Hunk};
use helix_view::document::{from_reader, to_writer};
use helix_view::{current_ref, Editor};

use super::super::review::{diff_hunks, index_changed};
use crate::compositor;
use crate::ui::PromptEvent;

pub(super) fn stage_hunk(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let target = Target::focused(cx.editor)?;
    let index_bytes =
        index::get_index_base(&target.path, true).map_err(|err| anyhow!("{err:#}"))?;
    let (index_text, bom) = target.decode(index_bytes.as_deref())?;
    let hunks = diff_hunks(&index_text, &target.text);
    let picked: Vec<&Hunk> = hunks
        .iter()
        .filter(|hunk| under(&hunk.after, &target.lines))
        .collect();
    if picked.is_empty() {
        bail!("There are no unstaged changes under any selection");
    }

    let staged = splice(
        &index_text,
        &target.text,
        picked
            .iter()
            .map(|hunk| (hunk.before.clone(), hunk.after.clone())),
    );
    // Staging all of a deleted file removes it from the index.
    let remove = staged.len_bytes() == 0 && !target.path.exists();
    target.write(cx.editor, (!remove).then_some((&staged, bom)))?;
    cx.editor
        .set_status(format!("Staged {}", changes(picked.len())));
    Ok(())
}

pub(super) fn unstage_hunk(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let target = Target::focused(cx.editor)?;
    let head_bytes = cx.editor.diff_providers.get_diff_base(&target.path, true);
    let index_bytes =
        index::get_index_base(&target.path, true).map_err(|err| anyhow!("{err:#}"))?;
    let (head, _) = target.decode(head_bytes.as_deref())?;
    let (index_text, bom) = target.decode(index_bytes.as_deref())?;
    // Staged hunks may not be on screen, so find them through the lines of
    // `HEAD` that the selections show.
    let shown = diff_hunks(&head, &target.text);
    let lines: Vec<(usize, usize)> = target
        .lines
        .iter()
        .map(|&lines| map_to_before(&shown, lines))
        .collect();
    let staged = diff_hunks(&head, &index_text);
    let picked: Vec<&Hunk> = staged
        .iter()
        .filter(|hunk| under(&hunk.before, &lines))
        .collect();
    if picked.is_empty() {
        bail!("There are no staged changes under any selection");
    }

    let unstaged = splice(
        &index_text,
        &head,
        picked
            .iter()
            .map(|hunk| (hunk.after.clone(), hunk.before.clone())),
    );
    // Unstaging all of a file that `HEAD` doesn't have removes it from the index.
    let remove = unstaged.len_bytes() == 0 && head_bytes.is_none();
    target.write(cx.editor, (!remove).then_some((&unstaged, bom)))?;
    cx.editor
        .set_status(format!("Unstaged {}", changes(picked.len())));
    Ok(())
}

/// The focused buffer, which both commands diff against the index.
struct Target {
    path: PathBuf,
    text: Rope,
    encoding: &'static Encoding,
    /// The selections' lines, as `line_ranges` gives them.
    lines: Vec<(usize, usize)>,
}

impl Target {
    fn focused(editor: &Editor) -> anyhow::Result<Self> {
        let (view, doc) = current_ref!(editor);
        let path = doc.path().context("The buffer has no file")?.to_path_buf();
        if !super::doc_trust_full(editor) {
            bail!("Staging runs git, which needs a trusted workspace. See :workspace-trust");
        }
        let text = doc.text().clone();
        let lines = doc.selection(view.id).line_ranges(text.slice(..)).collect();
        Ok(Self {
            path,
            text,
            encoding: doc.encoding(),
            lines,
        })
    }

    /// The text of a version of the file, and whether it has a byte order mark.
    /// A missing version is empty.
    fn decode(&self, bytes: Option<&[u8]>) -> anyhow::Result<(Rope, bool)> {
        let (text, _, bom) = from_reader(&mut bytes.unwrap_or_default(), Some(self.encoding))?;
        Ok((text, bom))
    }

    /// Makes `text` the index version of the file, or removes the file from the
    /// index when `text` is `None`.
    fn write(&self, editor: &mut Editor, text: Option<(&Rope, bool)>) -> anyhow::Result<()> {
        let bytes = match text {
            Some((text, bom)) => {
                let mut bytes = Vec::new();
                helix_lsp::block_on(to_writer(&mut bytes, (self.encoding, bom), text))?;
                Some(bytes)
            }
            None => None,
        };
        index::set_index_text(&self.path, bytes.as_deref(), true)
            .map_err(|err| anyhow!("{err:#}"))?;
        index_changed(editor);
        Ok(())
    }
}

fn changes(count: usize) -> String {
    match count {
        1 => "1 change".to_string(),
        _ => format!("{count} changes"),
    }
}

/// Whether the hunk side `lines` is under one of the selections' `ranges`,
/// picked the way `:reset-diff-change` picks hunks. So the line just below a
/// hunk picks it too, which is where removed rows show.
fn under(lines: &Range<u32>, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| lines.end as usize >= start && lines.start as usize <= end)
}

/// The `before` lines that the `after` lines `start..=end` map to through
/// `hunks`. A line inside a hunk maps to all of the hunk's `before` lines, and
/// an added line to the line its hunk comes before.
fn map_to_before(hunks: &[Hunk], (start, end): (usize, usize)) -> (usize, usize) {
    let map = |line: usize| {
        let preceding = hunks.partition_point(|hunk| hunk.after.start as usize <= line);
        let Some(hunk) = preceding.checked_sub(1).map(|i| &hunks[i]) else {
            return (line, line);
        };
        let (before, after) = (
            hunk.before.start as usize..hunk.before.end as usize,
            hunk.after.start as usize..hunk.after.end as usize,
        );
        if after.contains(&line) {
            (before.start, before.end.max(before.start + 1) - 1)
        } else {
            let line = line - after.end + before.end;
            (line, line)
        }
    };
    (map(start).0, map(end).1)
}

/// `from`, with the lines of each `(from_lines, to_lines)` pair replaced by
/// those lines of `to`. The pairs are in order and don't overlap.
fn splice(from: &Rope, to: &Rope, pairs: impl Iterator<Item = (Range<u32>, Range<u32>)>) -> Rope {
    fn lines(text: &Rope, lines: Range<u32>) -> RopeSlice<'_> {
        let start = text.line_to_char(lines.start as usize);
        let end = text.line_to_char(lines.end as usize);
        text.slice(start..end)
    }
    let mut out = String::new();
    let mut cursor = 0;
    for (from_lines, to_lines) in pairs {
        out.extend(lines(from, cursor..from_lines.start).chunks());
        out.extend(lines(to, to_lines).chunks());
        cursor = from_lines.end;
    }
    out.extend(lines(from, cursor..from.len_lines() as u32).chunks());
    Rope::from(out)
}

#[cfg(test)]
mod test {
    use helix_core::Rope;
    use helix_vcs::Hunk;

    use super::{map_to_before, splice, under};
    use crate::commands::review::diff_hunks;

    fn hunk(before: std::ops::Range<u32>, after: std::ops::Range<u32>) -> Hunk {
        Hunk { before, after }
    }

    #[test]
    fn splice_takes_only_the_picked_hunks() {
        let index = Rope::from("a\nb\nc\nd\n");
        let buffer = Rope::from("a\nB\nc\nD\ne\n");
        let hunks = diff_hunks(&index, &buffer);
        assert_eq!(hunks, [hunk(1..2, 1..2), hunk(3..4, 3..5)]);

        let staged = splice(
            &index,
            &buffer,
            hunks[1..]
                .iter()
                .map(|hunk| (hunk.before.clone(), hunk.after.clone())),
        );
        assert_eq!(staged, "a\nb\nc\nD\ne\n");
    }

    #[test]
    fn splice_handles_a_last_line_without_a_newline() {
        let index = Rope::from("a\nb");
        let buffer = Rope::from("a\nb\nc");
        let hunks = diff_hunks(&index, &buffer);
        let staged = splice(
            &index,
            &buffer,
            hunks
                .iter()
                .map(|hunk| (hunk.before.clone(), hunk.after.clone())),
        );
        assert_eq!(staged, "a\nb\nc");
    }

    #[test]
    fn a_hunk_is_under_its_lines_and_the_line_below() {
        // A cursor on the line below removed rows picks their hunk.
        assert!(under(&(3..3), &[(3, 3)]));
        assert!(under(&(1..3), &[(3, 3)]));
        assert!(!under(&(4..6), &[(3, 3)]));
        assert!(under(&(4..6), &[(0, 1), (5, 5)]));
    }

    #[test]
    fn lines_map_to_the_before_side() {
        // Line 1 replaced by two lines, line 4 removed.
        let hunks = [hunk(1..2, 1..3), hunk(4..5, 5..5)];
        // Unchanged lines shift by the hunks before them.
        assert_eq!(map_to_before(&hunks, (0, 0)), (0, 0));
        assert_eq!(map_to_before(&hunks, (3, 3)), (2, 2));
        // A changed line maps to the lines it replaced.
        assert_eq!(map_to_before(&hunks, (2, 2)), (1, 1));
        // The line below removed rows maps past them.
        assert_eq!(map_to_before(&hunks, (5, 5)), (5, 5));
        assert_eq!(map_to_before(&hunks, (0, 3)), (0, 2));
        // An added line maps to the line it comes before.
        assert_eq!(map_to_before(&[hunk(2..2, 2..4)], (3, 3)), (2, 2));
    }
}
