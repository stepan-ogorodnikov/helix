//! Maps an edit of the stream onto the files whose rows it touches. Pure: the
//! stream and the edit in, each file's changes in its own chars out, or why the
//! edit can't be made.

use helix_core::line_ending::{get_line_ending, LineEnding};
use helix_core::Change;
use helix_core::ChangeSet;

use super::{Content, Row, Stream};

/// Why an edit of the stream is put back.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// It touches a header, a gap, or a removed row.
    NotEditable,
    /// It adds or removes a line.
    Lines,
}

impl Refusal {
    pub(super) fn message(&self) -> &'static str {
        match self {
            Refusal::NotEditable => "Removed lines, headers, and gaps can't be edited",
            Refusal::Lines => "The review can't add or remove lines yet",
        }
    }
}

/// One file's share of an edit.
#[derive(Debug, PartialEq)]
pub(super) struct Edit {
    pub file: usize,
    pub changes: Vec<Change>,
}

// INVARIANT: `changes` apply to the stream's `text`, which the rows describe.
pub(super) fn edits(stream: &Stream, changes: &ChangeSet) -> Result<Vec<Edit>, Refusal> {
    let text = &stream.text;
    let editable = |row: usize| {
        matches!(
            stream.rows.get(row),
            Some(Row::Context { .. } | Row::Added { .. })
        )
    };
    let mut edits: Vec<Edit> = Vec::new();
    for (from, to, insert) in changes.changes_iter() {
        let row = text.char_to_line(from);
        let last = match to > from {
            true => text.char_to_line(to - 1),
            false => row,
        };
        let breaks = insert
            .as_ref()
            .is_some_and(|insert| insert.chars().any(|c| LineEnding::from_char(c).is_some()));
        if !breaks && !(row..=last).all(editable) {
            return Err(Refusal::NotEditable);
        }
        let start = text.line_to_char(row);
        let line = text.line(row);
        let end = start + line.len_chars() - get_line_ending(&line).map_or(0, |e| e.len_chars());
        if breaks || to > end {
            return Err(Refusal::Lines);
        }

        let (Row::Context { file, source, .. } | Row::Added { file, source }) = stream.rows[row]
        else {
            unreachable!("checked editable above");
        };
        let Content::Text { source: rope, .. } = &stream.files[file].content else {
            unreachable!("only a text file has rows below its header");
        };
        // INVARIANT: a row's chars match its source line's one to one, so an
        // offset into the row is an offset into the line.
        let at = |pos: usize| rope.line_to_char(source) + pos - start;
        let change = (at(from), at(to), insert);
        match edits.last_mut() {
            Some(edit) if edit.file == file => edit.changes.push(change),
            _ => edits.push(Edit {
                file,
                changes: vec![change],
            }),
        }
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use helix_core::{Rope, Transaction};
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

    /// The changes the stream's edit makes to each file, as the files' texts
    /// after it.
    fn edited(stream: &Stream, change: impl FnOnce(&Rope) -> Transaction) -> Vec<String> {
        let transaction = change(&stream.text);
        let edits = edits(stream, transaction.changes()).unwrap();
        edits
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

    fn at(text: &Rope, row: usize, column: usize) -> usize {
        text.line_to_char(row) + column
    }

    #[test]
    fn an_edit_within_a_row_lands_on_its_line_of_the_file() {
        let stream = two_files();
        #[rustfmt::skip]
        let cases: &[(&str, usize, usize, usize, &str, &str)] = &[
            ("typing in a context row", 1, 3, 3, "!", "one!\nTWO\nthree\n"),
            ("typing at the start of an added row", 3, 0, 0, ">", "one\n>TWO\nthree\n"),
            ("deleting a whole row's text keeps its line", 4, 0, 5, "", "one\nTWO\n\n"),
            ("a row with no line ending in its file", 7, 1, 1, "z", "yz"),
        ];
        for &(case, row, from, to, insert, file) in cases {
            let insert = (!insert.is_empty()).then(|| insert.into());
            let got = edited(&stream, |text| {
                let change = (at(text, row, from), at(text, row, to), insert);
                Transaction::change(text, std::iter::once(change))
            });
            assert_eq!(got, [file], "{case}");
        }
    }

    #[test]
    fn one_edit_in_two_files_gives_each_file_its_share() {
        let stream = two_files();
        let got = edited(&stream, |text| {
            let changes =
                [1, 4, 7].map(|row| (at(text, row, 0), at(text, row, 0), Some("+".into())));
            Transaction::change(text, changes.into_iter())
        });
        assert_eq!(got, ["+one\nTWO\n+three\n", "+y"]);
    }

    #[test]
    fn an_edit_outside_the_rows_of_a_files_lines_is_refused() {
        let stream = two_files();
        #[rustfmt::skip]
        let cases: &[(&str, usize, usize, usize, usize, &str, Refusal)] = &[
            ("typing in a header", 0, 0, 0, 0, "x", Refusal::NotEditable),
            ("deleting a removed row's text", 2, 0, 2, 3, "", Refusal::NotEditable),
            ("deleting from an added row into the removed row above", 2, 1, 3, 1, "", Refusal::NotEditable),
            ("typing past the last row", 8, 0, 8, 0, "x", Refusal::NotEditable),
            ("joining two rows of the file", 3, 3, 4, 0, "", Refusal::Lines),
            ("deleting a row's line ending", 1, 3, 2, 0, "", Refusal::Lines),
            ("a new line inside a row", 4, 2, 4, 2, "\n", Refusal::Lines),
            ("a new line where a removed row ends, as O does", 2, 3, 2, 3, "\n", Refusal::Lines),
        ];
        for (case, from_row, from, to_row, to, insert, refusal) in cases {
            let text = &stream.text;
            let insert = (!insert.is_empty()).then(|| (*insert).into());
            let change = (at(text, *from_row, *from), at(text, *to_row, *to), insert);
            let transaction = Transaction::change(text, std::iter::once(change));
            assert_eq!(
                edits(&stream, transaction.changes()).as_ref(),
                Err(refusal),
                "{case}"
            );
        }
    }
}
