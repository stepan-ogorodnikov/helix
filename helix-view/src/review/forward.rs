//! Maps an edit of the stream onto the files whose rows it touches. Pure: the
//! stream and the edit in; each file's changes in its own chars, and the rows of
//! the stream's new text, out. Or why the edit can't be made.
//!
//! An edit stays inside one run: a stretch of added and unchanged rows, which
//! stands for a stretch of one file's lines.

use std::ops::Range;

use helix_core::line_ending::{get_line_ending, LineEnding};
use helix_core::{Assoc, Change, ChangeSet, Rope, Transaction};

use super::{Content, Row, Stream};

/// Why an edit of the stream is put back.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// It touches a header, a gap, or a removed row.
    NotEditable,
    /// It leaves a run without the line ending that ends it, so its last line
    /// would join the row below.
    Join,
}

impl Refusal {
    pub(super) fn message(&self) -> &'static str {
        match self {
            Refusal::NotEditable => "Removed lines, headers, and gaps can't be edited",
            Refusal::Join => "That would join a line to a removed line, a header, or a gap",
        }
    }
}

/// One file's share of an edit.
#[derive(Debug, PartialEq)]
pub(super) struct Edit {
    pub file: usize,
    pub changes: Vec<Change>,
}

pub(super) struct Forward {
    pub edits: Vec<Edit>,
    /// The rows of the stream's text after the edit, keyed by the files' new
    /// lines.
    pub rows: Vec<Row>,
    /// The stream's text after the edit.
    pub text: Rope,
}

/// Rows `rows` of `file`, which hold chars `chars` of the stream and show the
/// file's lines from `line` on.
struct Run {
    file: usize,
    rows: Range<usize>,
    chars: Range<usize>,
    line: usize,
}

fn runs(stream: &Stream) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (row, kind) in stream.rows.iter().enumerate() {
        let (Row::Context { file, source, .. } | Row::Added { file, source }) = *kind else {
            continue;
        };
        // INVARIANT: a header separates two files' rows, so a run never spans
        // files, and its rows show consecutive lines.
        match runs.last_mut() {
            Some(run) if run.rows.end == row => run.rows.end += 1,
            _ => runs.push(Run {
                file,
                rows: row..row + 1,
                chars: 0..0,
                line: source,
            }),
        }
    }
    for run in &mut runs {
        run.chars =
            stream.text.line_to_char(run.rows.start)..stream.text.line_to_char(run.rows.end);
    }
    runs
}

// WHY: `O` on a run's first row inserts a line ending and indent where the row
// above it ends, and that row isn't editable. It's the same text as the line
// inserted where the run starts.
fn above_run(stream: &Stream, (from, to, insert): Change) -> Change {
    let text = &stream.text;
    let row = text.char_to_line(from);
    let editable = |row: usize| {
        matches!(
            stream.rows.get(row),
            Some(Row::Context { .. } | Row::Added { .. })
        )
    };
    let line = text.line(row);
    let (Some(inserted), Some(ending)) = (insert.as_deref(), get_line_ending(&line)) else {
        return (from, to, insert);
    };
    let end = text.line_to_char(row) + line.len_chars() - ending.len_chars();
    if from != to || from != end || editable(row) || !editable(row + 1) {
        return (from, to, insert);
    }
    let Some(rest) = inserted.strip_prefix(ending.as_str()) else {
        return (from, to, insert);
    };
    let start = text.line_to_char(row + 1);
    (
        start,
        start,
        Some(format!("{rest}{}", ending.as_str()).into()),
    )
}

// INVARIANT: `changes` apply to the stream's `text`, which the rows describe.
pub(super) fn forward(stream: &Stream, changes: &ChangeSet) -> Result<Forward, Refusal> {
    let text = &stream.text;
    let runs = runs(stream);
    let mut owned: Vec<(usize, Change)> = Vec::new();
    for change in changes.changes_iter() {
        let (from, to, insert) = above_run(stream, change);
        let run = runs.partition_point(|run| run.chars.end < from);
        let inside = runs
            .get(run)
            .is_some_and(|run| run.chars.start <= from && to <= run.chars.end);
        let ordered = owned.last().is_none_or(|(_, (_, last, _))| *last <= from);
        if !inside || !ordered {
            return Err(Refusal::NotEditable);
        }
        owned.push((run, (from, to, insert)));
    }

    let moved = Transaction::change(text, owned.iter().map(|(_, change)| change.clone()));
    let mut after = text.clone();
    moved.apply(&mut after);
    // The chars each run holds in the new text. An insert at a run's start or
    // end is the run's.
    let spans: Vec<Range<usize>> = runs
        .iter()
        .map(|run| {
            let changes = moved.changes();
            changes.map_pos(run.chars.start, Assoc::Before)
                ..changes.map_pos(run.chars.end, Assoc::After)
        })
        .collect();

    let mut edits: Vec<Edit> = Vec::new();
    for (index, run) in runs.iter().enumerate() {
        let changes: Vec<&Change> = owned
            .iter()
            .filter(|(owner, _)| *owner == index)
            .map(|(_, change)| change)
            .collect();
        if changes.is_empty() {
            continue;
        }
        let span = &spans[index];
        let ends_line = |pos: usize| LineEnding::from_char(after.char(pos)).is_some();
        if !span.is_empty() && !ends_line(span.end - 1) {
            return Err(Refusal::Join);
        }
        let changes = file_changes(stream, run, &changes);
        match edits.last_mut() {
            Some(edit) if edit.file == run.file => edit.changes.extend(changes),
            _ => edits.push(Edit {
                file: run.file,
                changes,
            }),
        }
    }

    let rows = rows(stream, &runs, &spans, &after);
    Ok(Forward {
        edits,
        rows,
        text: after,
    })
}

/// A run's changes in its file's chars.
fn file_changes(stream: &Stream, run: &Run, changes: &[&Change]) -> Vec<Change> {
    let text = &stream.text;
    let Content::Text { source, .. } = &stream.files[run.file].content else {
        unreachable!("only a text file has rows below its header");
    };
    let start = source.line_to_char(run.line);
    let last_row = run.rows.end - 1;
    let last_line = run.line + run.rows.len() - 1;
    // WHY: a file's last line with no line ending shows one, which stands for
    // the end of the file. An edit that reaches that line makes it real, so an
    // emptied or new last line still has a line of its own.
    let shown = get_line_ending(&text.line(last_row));
    let virtual_ending = shown.filter(|_| get_line_ending(&source.line(last_line)).is_none());
    let end_of_file = virtual_ending.map(|ending| run.chars.end - ending.len_chars());
    let at = |pos: usize| start + end_of_file.map_or(pos, |end| pos.min(end)) - run.chars.start;

    let mut ending = virtual_ending.filter(|_| {
        let last_row_start = text.line_to_char(last_row);
        let reaches = changes.iter().any(|(_, to, _)| *to >= last_row_start);
        let deleted = changes
            .iter()
            .any(|(from, to, _)| from < to && Some(*from) <= end_of_file && *to == run.chars.end);
        reaches && !deleted
    });
    let mut mapped = Vec::with_capacity(changes.len() + 1);
    for (from, to, insert) in changes {
        // INVARIANT: the line ending goes after an insert where the file ends,
        // and before one after the line ending the stream shows.
        if *from == run.chars.end {
            if let Some(ending) = ending.take() {
                mapped.push((at(*from), at(*from), Some(ending.as_str().into())));
            }
        }
        mapped.push((at(*from), at(*to), insert.clone()));
    }
    if let Some(ending) = ending {
        let end = source.len_chars();
        mapped.push((end, end, Some(ending.as_str().into())));
    }
    mapped
}

/// The rows of `after`, the stream's text after the edit. A run whose line
/// count changed shows its new lines as added until the relayout.
fn rows(stream: &Stream, runs: &[Run], spans: &[Range<usize>], after: &Rope) -> Vec<Row> {
    let shift = |row: Row, delta: isize| {
        let by = |line: usize| line.saturating_add_signed(delta);
        match row {
            Row::Gap { file, start, end } => Row::Gap {
                file,
                start: by(start),
                end: by(end),
            },
            Row::Context { file, base, source } => Row::Context {
                file,
                base,
                source: by(source),
            },
            Row::Added { file, source } => Row::Added {
                file,
                source: by(source),
            },
            Row::Header { .. } | Row::Removed { .. } => row,
        }
    };
    let mut rows = Vec::with_capacity(stream.rows.len());
    // How many lines the file's edited runs so far added.
    let mut delta: isize = 0;
    let mut runs = runs.iter().zip(spans).peekable();
    let mut row = 0;
    while row < stream.rows.len() {
        if let Some((run, span)) = runs.next_if(|(run, _)| run.rows.start == row) {
            let lines = after.char_to_line(span.end) - after.char_to_line(span.start);
            if lines == run.rows.len() {
                rows.extend(run.rows.clone().map(|row| shift(stream.rows[row], delta)));
            } else {
                let first = run.line.saturating_add_signed(delta);
                rows.extend((first..first + lines).map(|source| Row::Added {
                    file: run.file,
                    source,
                }));
            }
            delta += lines as isize - run.rows.len() as isize;
            row = run.rows.end;
            continue;
        }
        if let Row::Header { .. } = stream.rows[row] {
            delta = 0;
        }
        rows.push(shift(stream.rows[row], delta));
        row += 1;
    }
    rows
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use helix_vcs::FileChange;

    use super::*;
    use crate::review::tests::stream_of;
    use crate::review::{File, Section};

    fn file(name: &str, base: &str, source: &str) -> File {
        File::from(Section {
            change: FileChange::Modified {
                path: PathBuf::from(name),
            },
            content: Content::Text {
                base: Rope::from(base),
                source: Rope::from(source),
            },
            encoding: helix_core::encoding::UTF_8,
        })
    }

    /// Two files as the stream shows them:
    ///
    /// ```text
    /// 0  a.txt  modified
    /// 1  one          context, a.txt line 0
    /// 2  two          removed
    /// 3  TWO          added, a.txt line 1
    /// 4  three        context, a.txt line 2
    /// 5  b.txt  modified
    /// 6  x            removed
    /// 7  y            added, b.txt line 0, with no line ending in the file
    /// ```
    fn two_files() -> Stream {
        stream_of(vec![
            file("a.txt", "one\ntwo\nthree\n", "one\nTWO\nthree\n"),
            file("b.txt", "x\n", "y"),
        ])
    }

    /// A row, and a column in it.
    type Place = (usize, usize);

    /// The char at `column` of `row`, or where the row's text ends for `END`.
    fn at(text: &Rope, (row, column): Place) -> usize {
        let line = text.line(row);
        let end = line.len_chars() - get_line_ending(&line).map_or(0, |e| e.len_chars());
        text.line_to_char(row) + column.min(end)
    }

    const END: usize = usize::MAX;

    fn edit(stream: &Stream, from: Place, to: Place, insert: &str) -> Transaction {
        let text = &stream.text;
        let insert = (!insert.is_empty()).then(|| insert.into());
        Transaction::change(
            text,
            std::iter::once((at(text, from), at(text, to), insert)),
        )
    }

    /// Each edited file's text after the edit.
    fn edited(stream: &Stream, transaction: &Transaction) -> Vec<String> {
        let forward = forward(stream, transaction.changes()).unwrap();
        let mut after = stream.text.clone();
        transaction.apply(&mut after);
        assert_eq!(forward.text, after);
        forward
            .edits
            .into_iter()
            .map(|edit| {
                let Content::Text { source, .. } = &stream.files[edit.file].content else {
                    unreachable!()
                };
                let mut text = source.clone();
                Transaction::change(source, edit.changes.into_iter()).apply(&mut text);
                text.to_string()
            })
            .collect()
    }

    #[test]
    fn an_edit_within_a_run_lands_on_its_lines_of_the_file() {
        let stream = two_files();
        #[rustfmt::skip]
        let cases: &[(&str, Place, Place, &str, &str)] = &[
            ("typing in a context row", (1, 3), (1, 3), "!", "one!\nTWO\nthree\n"),
            ("typing at the start of an added row", (3, 0), (3, 0), ">", "one\n>TWO\nthree\n"),
            ("deleting a whole row's text keeps its line", (4, 0), (4, END), "", "one\nTWO\n\n"),
            ("joining two rows of a run", (3, END), (4, 0), "", "one\nTWOthree\n"),
            ("a new line inside a row", (4, 2), (4, 2), "\n", "one\nTWO\nth\nree\n"),
            ("deleting a whole row", (4, 0), (5, 0), "", "one\nTWO\n"),
            ("a line pasted where a run ends", (5, 0), (5, 0), "x\n", "one\nTWO\nthree\nx\n"),
            ("O below a removed row", (2, END), (2, END), "\n", "one\n\nTWO\nthree\n"),
            ("O below a header", (0, END), (0, END), "\n  ", "  \none\nTWO\nthree\n"),
            ("typing on a last line with no line ending gives it one", (7, 1), (7, 1), "z", "yz\n"),
            ("o on that line", (7, END), (7, END), "\n", "y\n\n"),
            ("a line pasted after it", (8, 0), (8, 0), "z\n", "y\nz\n"),
            ("deleting that line", (7, 0), (8, 0), "", ""),
        ];
        for &(case, from, to, insert, file) in cases {
            let transaction = edit(&stream, from, to, insert);
            assert_eq!(edited(&stream, &transaction), [file], "{case}");
        }
    }

    #[test]
    fn one_edit_in_two_files_gives_each_file_its_share() {
        let stream = two_files();
        let text = &stream.text;
        let changes = [1, 4, 7].map(|row| {
            let start = text.line_to_char(row);
            (start, start, Some("+".into()))
        });
        let transaction = Transaction::change(text, changes.into_iter());
        assert_eq!(
            edited(&stream, &transaction),
            ["+one\nTWO\n+three\n", "+y\n"]
        );
    }

    #[test]
    fn an_edit_that_leaves_its_run_is_refused() {
        let stream = two_files();
        #[rustfmt::skip]
        let cases: &[(&str, Place, Place, &str, Refusal)] = &[
            ("typing in a header", (0, 0), (0, 0), "x", Refusal::NotEditable),
            ("deleting a removed row's text", (2, 0), (2, END), "", Refusal::NotEditable),
            ("deleting from an added row into the removed row above", (2, 1), (3, 1), "", Refusal::NotEditable),
            ("deleting the line ending above a run", (2, END), (3, 0), "", Refusal::NotEditable),
            ("deleting the line ending that ends a run", (4, END), (5, 0), "", Refusal::Join),
            ("typing where a run ends", (5, 0), (5, 0), "x", Refusal::Join),
            ("typing past the last row", (8, 0), (8, 0), "x", Refusal::Join),
        ];
        for (case, from, to, insert, refusal) in cases {
            let transaction = edit(&stream, *from, *to, insert);
            assert_eq!(
                forward(&stream, transaction.changes()).err().as_ref(),
                Some(refusal),
                "{case}"
            );
        }
    }

    #[test]
    fn rows_follow_the_lines_an_edit_adds() {
        let stream = stream_of(vec![file(
            "a.txt",
            "a\nb\nc\nd\ne\nf\ng\n",
            "A\nb\nc\nd\ne\nf\ng\n",
        )]);
        let transaction = edit(&stream, (3, 1), (3, 1), "\n");
        let forward = forward(&stream, transaction.changes()).unwrap();
        let added = |source| Row::Added { file: 0, source };
        assert_eq!(
            forward.rows,
            [
                Row::Header { file: 0 },
                Row::Removed { file: 0, base: 0 },
                added(0),
                added(1),
                added(2),
                added(3),
                added(4),
                Row::Gap {
                    file: 0,
                    start: 5,
                    end: 8
                },
            ],
            "the run's lines come up as added, and the gap below moves down a line"
        );
    }
}
