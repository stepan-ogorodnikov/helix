//! `:stage-hunk` and `:unstage-hunk` write the hunks the selection touches in
//! the review to the index, whole, through `git`. Ported from diff review,
//! which took the lines from a file's buffer instead of the review's rows.

use std::ops::Range;

use anyhow::{anyhow, bail, Context as _};
use helix_core::command_line::Args;
use helix_core::encoding::Encoding;
use helix_core::{Rope, RopeSlice};
use helix_loader::workspace_trust::TrustQuery;
use helix_vcs::{index, Hunk};
use helix_view::current_ref;
use helix_view::document::{from_reader, to_writer};
use helix_view::review::{layout, Picked};

use super::{open_stream, read_changes};
use crate::compositor;
use crate::ui::PromptEvent;

pub(in crate::commands) fn stage_hunk(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let staged = write_picked(cx, "Staged", |file, trust_full| {
        let path = file.change.path();
        let index_bytes =
            index::get_index_base(path, trust_full).map_err(|err| anyhow!("{err:#}"))?;
        let (index_text, bom) = decode(file.encoding, index_bytes.as_deref())?;
        let hunks = hunks(&index_text, file.source)?;
        let picked: Vec<&Hunk> = hunks
            .iter()
            .filter(|hunk| under(&hunk.after, &file.lines))
            .collect();
        if picked.is_empty() {
            return Ok(0);
        }
        let staged = splice(
            &index_text,
            file.source,
            picked
                .iter()
                .map(|hunk| (hunk.before.clone(), hunk.after.clone())),
        );
        // WHY: staging all of a deleted file removes it from the index.
        let remove = staged.len_bytes() == 0 && !path.exists();
        write(file, (!remove).then_some((&staged, bom)), trust_full)?;
        Ok(picked.len())
    })?;
    if staged == 0 {
        bail!("There are no unstaged changes under any selection");
    }
    Ok(())
}

pub(in crate::commands) fn unstage_hunk(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let providers = cx.editor.diff_providers.clone();
    let unstaged = write_picked(cx, "Unstaged", |file, trust_full| {
        let path = file.change.path();
        let head_bytes = providers.get_diff_base(path, trust_full);
        let index_bytes =
            index::get_index_base(path, trust_full).map_err(|err| anyhow!("{err:#}"))?;
        let (head, _) = decode(file.encoding, head_bytes.as_deref())?;
        let (index_text, bom) = decode(file.encoding, index_bytes.as_deref())?;
        // WHY: the rows show the source, so their lines go through the
        // `HEAD`-to-source diff to `HEAD`'s, where staged hunks start.
        let shown = hunks(&head, file.source)?;
        let lines: Vec<(usize, usize)> = file
            .lines
            .iter()
            .map(|&lines| map_to_before(&shown, lines))
            .collect();
        let staged = hunks(&head, &index_text)?;
        let picked: Vec<&Hunk> = staged
            .iter()
            .filter(|hunk| under(&hunk.before, &lines))
            .collect();
        if picked.is_empty() {
            return Ok(0);
        }
        let unstaged = splice(
            &index_text,
            &head,
            picked
                .iter()
                .map(|hunk| (hunk.after.clone(), hunk.before.clone())),
        );
        // WHY: unstaging all of a file that `HEAD` doesn't have removes it
        // from the index.
        let remove = unstaged.len_bytes() == 0 && head_bytes.is_none();
        write(file, (!remove).then_some((&unstaged, bom)), trust_full)?;
        Ok(picked.len())
    })?;
    if unstaged == 0 {
        bail!("There are no staged changes under any selection");
    }
    Ok(())
}

/// Calls `write` for each file with hunks under the focused review's
/// selection, then reads the review again and says what `verb` did. Returns
/// the changes written.
fn write_picked(
    cx: &mut compositor::Context,
    verb: &str,
    write: impl Fn(&Picked, bool) -> anyhow::Result<usize>,
) -> anyhow::Result<usize> {
    let (view, doc) = current_ref!(cx.editor);
    let stream = open_stream(cx.editor)
        .filter(|stream| stream.doc == doc.id())
        .context("This buffer isn't a review. Start one with :review")?;
    let trust_full = cx
        .editor
        .workspace_trust
        .query(
            &helix_loader::find_workspace_in(&stream.root).0,
            TrustQuery::Git,
        )
        .is_trusted();
    if !trust_full {
        bail!("Staging runs git, which needs a trusted workspace. See :workspace-trust");
    }
    let rows: Vec<_> = doc
        .selection(view.id)
        .line_ranges(doc.text().slice(..))
        .collect();
    let mut written = 0;
    for file in stream.picked(&rows) {
        written += write(&file, trust_full).with_context(|| {
            let path = helix_stdx::path::get_relative_path(file.change.path());
            format!("{}", path.display())
        })?;
    }
    if written > 0 {
        let scope = stream.scope;
        read_changes(cx, scope, Some(format!("{verb} {}", changes(written))));
    }
    Ok(written)
}

fn hunks(from: &Rope, to: &Rope) -> anyhow::Result<Vec<Hunk>> {
    layout::hunks(from, to).context("The file is too large to diff")
}

/// The text of a version of the file, and whether it has a byte order mark.
/// A missing version is empty.
fn decode(encoding: &'static Encoding, bytes: Option<&[u8]>) -> anyhow::Result<(Rope, bool)> {
    let (text, _, bom) = from_reader(&mut bytes.unwrap_or_default(), Some(encoding))?;
    Ok((text, bom))
}

/// Makes `text` the index version of the file, or removes the file from the
/// index when `text` is `None`.
fn write(file: &Picked, text: Option<(&Rope, bool)>, trust_full: bool) -> anyhow::Result<()> {
    let bytes = match text {
        Some((text, bom)) => {
            let mut bytes = Vec::new();
            helix_lsp::block_on(to_writer(&mut bytes, (file.encoding, bom), text))?;
            Some(bytes)
        }
        None => None,
    };
    index::set_index_text(file.change.path(), bytes.as_deref(), trust_full)
        .map_err(|err| anyhow!("{err:#}"))
}

fn changes(count: usize) -> String {
    match count {
        1 => "1 change".to_string(),
        _ => format!("{count} changes"),
    }
}

/// Whether the hunk side `lines` touches one of `ranges`, first and last lines,
/// or ends just above it. A removal has no lines, so it's under the line below.
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

    use super::{hunks, map_to_before, splice, under};

    fn hunk(before: std::ops::Range<u32>, after: std::ops::Range<u32>) -> Hunk {
        Hunk { before, after }
    }

    #[test]
    fn splice_takes_only_the_picked_hunks() {
        let index = Rope::from("a\nb\nc\nd\n");
        let buffer = Rope::from("a\nB\nc\nD\ne\n");
        let hunks = hunks(&index, &buffer).unwrap();
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
        let hunks = hunks(&index, &buffer).unwrap();
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
    fn a_removal_is_under_the_line_below_it() {
        assert!(under(&(3..3), &[(3, 3)]));
        assert!(under(&(1..3), &[(2, 2)]));
        assert!(under(&(1..3), &[(3, 3)]));
        assert!(!under(&(4..6), &[(2, 2)]));
        assert!(under(&(4..6), &[(0, 1), (5, 5)]));
    }

    #[test]
    fn lines_map_to_the_before_side() {
        let hunks = [hunk(1..2, 1..3), hunk(4..5, 5..5)];
        assert_eq!(
            map_to_before(&hunks, (0, 0)),
            (0, 0),
            "unchanged lines stay"
        );
        assert_eq!(
            map_to_before(&hunks, (3, 3)),
            (2, 2),
            "and shift past hunks"
        );
        assert_eq!(
            map_to_before(&hunks, (2, 2)),
            (1, 1),
            "a changed line maps to what it replaced"
        );
        assert_eq!(
            map_to_before(&hunks, (5, 5)),
            (5, 5),
            "the line below a removal maps past it"
        );
        assert_eq!(map_to_before(&hunks, (0, 3)), (0, 2));
        assert_eq!(
            map_to_before(&[hunk(2..2, 2..4)], (3, 3)),
            (2, 2),
            "an added line maps to the line it comes before"
        );
    }
}
