//! Which rows a changed file gets in the review stream. Pure: hunks and line
//! counts in, rows out.
//!
//! Each hunk gets `CONTEXT` lines around it, and hunks whose context would
//! touch merge, as in git. A gap row stands for each run of lines left out.

use helix_core::{Rope, RopeSlice};
use helix_vcs::Hunk;
use imara_diff::{Algorithm, Diff, IndentHeuristic, IndentLevel, InternedInput, Interner};

use super::Row;

/// Unchanged lines shown around each hunk, as in git.
pub const CONTEXT: usize = 3;

/// The diff worker's size limits. Past them a file gets no rows.
const MAX_DIFF_LINES: usize = 64 * u16::MAX as usize;
const MAX_DIFF_BYTES: usize = MAX_DIFF_LINES * 128;

// WHY: not `Rope::len_lines`, which counts the empty line after a final line
// ending as a line of its own.
pub fn line_count(text: &Rope) -> usize {
    let lines = text.len_lines();
    if text.line(lines - 1).len_chars() == 0 {
        lines - 1
    } else {
        lines
    }
}

fn lines(text: &Rope) -> impl Iterator<Item = RopeSlice<'_>> {
    text.lines().take(line_count(text))
}

pub fn hunks(base: &Rope, source: &Rope) -> Option<Vec<Hunk>> {
    let too_large =
        |text: &Rope| text.len_lines() > MAX_DIFF_LINES || text.len_bytes() > MAX_DIFF_BYTES;
    if too_large(base) || too_large(source) {
        return None;
    }
    let mut input = InternedInput {
        before: Vec::new(),
        after: Vec::new(),
        interner: Interner::new(base.len_lines() + source.len_lines()),
    };
    // WHY: not `Rope::lines`, which ends with an empty line after a final line
    // ending. A file that loses its final line ending would get a removed row
    // for that empty line.
    input.update_before(lines(base));
    input.update_after(lines(source));
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_with(
        &input.before,
        &input.after,
        IndentHeuristic::new(|token| IndentLevel::for_ascii_line(input.interner[token].bytes(), 4)),
    );
    Some(diff.hunks().collect())
}

pub fn rows(file: usize, hunks: &[Hunk], source_lines: usize, context: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut shown = 0;
    let mut first = 0;
    while first < hunks.len() {
        let mut last = first;
        while hunks
            .get(last + 1)
            .is_some_and(|next| next.after.start - hunks[last].after.end <= 2 * context as u32)
        {
            last += 1;
        }

        let hunk = &hunks[first];
        let mut source = (hunk.after.start as usize).saturating_sub(context);
        // INVARIANT: the lines before a hunk, back to the previous one, are the
        // same on both sides, so context lines have a base line too.
        let mut base = hunk.before.start as usize - (hunk.after.start as usize - source);
        if source > shown {
            rows.push(Row::Gap {
                file,
                start: shown,
                end: source,
            });
        }
        for hunk in &hunks[first..=last] {
            while source < hunk.after.start as usize {
                rows.push(Row::Context { file, base, source });
                base += 1;
                source += 1;
            }
            rows.extend(hunk.before.clone().map(|base| Row::Removed {
                file,
                base: base as usize,
            }));
            rows.extend(hunk.after.clone().map(|source| Row::Added {
                file,
                source: source as usize,
            }));
            base = hunk.before.end as usize;
            source = hunk.after.end as usize;
        }
        let end = (source + context).min(source_lines);
        while source < end {
            rows.push(Row::Context { file, base, source });
            base += 1;
            source += 1;
        }

        shown = end;
        first = last + 1;
    }
    if !hunks.is_empty() && shown < source_lines {
        rows.push(Row::Gap {
            file,
            start: shown,
            end: source_lines,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    //! Rows print as `-3` for removed base line 3, `+3` for added source line 3,
    //! `3:4` for context at base line 3 and source line 4, and `~2..5` for a gap.

    use super::*;

    fn hunk(before: std::ops::Range<u32>, after: std::ops::Range<u32>) -> Hunk {
        Hunk { before, after }
    }

    fn show(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match *row {
                Row::Header { .. } => "header".into(),
                Row::Gap { start, end, .. } => format!("~{start}..{end}"),
                Row::Removed { base, .. } => format!("-{base}"),
                Row::Context { base, source, .. } => format!("{base}:{source}"),
                Row::Added { source, .. } => format!("+{source}"),
            })
            .collect()
    }

    #[test]
    fn rows_show_hunks_with_context_and_gaps() {
        #[rustfmt::skip]
        let cases: &[(&str, &[Hunk], usize, &[&str])] = &[
            (
                "a change in the middle hides the lines around its context",
                &[hunk(10..11, 10..11)],
                30,
                &["~0..7", "7:7", "8:8", "9:9", "-10", "+10", "11:11", "12:12", "13:13", "~14..30"],
            ),
            (
                "no gap row when nothing is hidden",
                &[hunk(3..4, 3..5)],
                8,
                &["0:0", "1:1", "2:2", "-3", "+3", "+4", "4:5", "5:6", "6:7"],
            ),
            (
                "removed rows go above added rows",
                &[hunk(0..2, 0..1)],
                1,
                &["-0", "-1", "+0"],
            ),
            (
                "hunks six lines apart share their context",
                &[hunk(2..3, 2..3), hunk(9..10, 9..10)],
                20,
                &["0:0", "1:1", "-2", "+2", "3:3", "4:4", "5:5", "6:6", "7:7", "8:8", "-9", "+9",
                  "10:10", "11:11", "12:12", "~13..20"],
            ),
            (
                "hunks seven lines apart get a gap of one line",
                &[hunk(2..3, 2..3), hunk(10..11, 10..11)],
                20,
                &["0:0", "1:1", "-2", "+2", "3:3", "4:4", "5:5", "~6..7", "7:7", "8:8", "9:9",
                  "-10", "+10", "11:11", "12:12", "13:13", "~14..20"],
            ),
            (
                "context after an insertion follows the shifted lines on each side",
                &[hunk(1..1, 1..3), hunk(9..10, 11..11)],
                16,
                &["0:0", "+1", "+2", "1:3", "2:4", "3:5", "~6..8", "6:8", "7:9", "8:10", "-9",
                  "10:11", "11:12", "12:13", "~14..16"],
            ),
            (
                "a removal at the end has no context after it",
                &[hunk(5..7, 5..5)],
                5,
                &["~0..2", "2:2", "3:3", "4:4", "-5", "-6"],
            ),
            (
                "a new file is all added rows",
                &[hunk(0..0, 0..2)],
                2,
                &["+0", "+1"],
            ),
            (
                "a file with no hunks has no rows",
                &[],
                4,
                &[],
            ),
        ];
        for (name, hunks, source_lines, expected) in cases {
            assert_eq!(
                show(&rows(0, hunks, *source_lines, CONTEXT)),
                *expected,
                "{name}"
            );
        }
    }

    #[test]
    fn hunks_ignore_the_empty_line_after_a_final_line_ending() {
        let base = Rope::from("a\nb\n");
        assert_eq!(
            hunks(&base, &Rope::from("a\nb")),
            Some(vec![hunk(1..2, 1..2)])
        );
        assert_eq!(
            hunks(&base, &Rope::from("a\nb\nc\n")),
            Some(vec![hunk(2..2, 2..3)])
        );
        assert_eq!(hunks(&Rope::new(), &base), Some(vec![hunk(0..0, 0..2)]));
    }

    #[test]
    fn line_count_does_not_count_the_line_after_a_final_line_ending() {
        for (text, lines) in [
            ("", 0),
            ("a", 1),
            ("a\n", 1),
            ("a\nb", 2),
            ("a\r\nb\r\n", 2),
        ] {
            assert_eq!(line_count(&Rope::from(text)), lines, "{text:?}");
        }
    }
}
