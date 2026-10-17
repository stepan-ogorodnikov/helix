//! The review stream: one read-only scratch buffer that shows the hunks of
//! every changed file, one row per line.
//!
//! `helix-term` lists the changed files and reads both sides of each. This
//! module lays out the rows, owns the buffer's text, and keeps it read-only.

use std::collections::HashMap;
use std::path::Path;

use helix_core::{line_ending::get_line_ending, Rope, RopeSlice, Selection, Transaction};
use helix_vcs::FileChange;

use crate::editor::Action;
use crate::{align_view, Align, DocumentId, Editor, ViewId};

pub(crate) mod gutter;
pub mod layout;
mod sync;

pub use sync::sync;

/// What a line of the stream shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The path of `files[file]` and how it changed.
    Header { file: usize },
    /// Unchanged source lines `start..end` that aren't shown.
    Gap {
        file: usize,
        start: usize,
        end: usize,
    },
    /// A line of the base that the source no longer has.
    Removed { file: usize, base: usize },
    /// A line both sides have.
    Context {
        file: usize,
        base: usize,
        source: usize,
    },
    /// A line of the source that the base doesn't have.
    Added { file: usize, source: usize },
}

impl Row {
    pub fn file(self) -> usize {
        match self {
            Row::Header { file }
            | Row::Gap { file, .. }
            | Row::Removed { file, .. }
            | Row::Context { file, .. }
            | Row::Added { file, .. } => file,
        }
    }

    pub fn lines(self) -> (Option<usize>, Option<usize>) {
        match self {
            Row::Removed { base, .. } => (Some(base), None),
            Row::Context { base, source, .. } => (Some(base), Some(source)),
            Row::Added { source, .. } => (None, Some(source)),
            Row::Header { .. } | Row::Gap { .. } => (None, None),
        }
    }
}

/// A changed file, with both sides of it.
pub struct Section {
    pub change: FileChange,
    pub content: Content,
}

pub enum Content {
    /// `base` is empty for a new file, and `source` for a deleted one.
    Text {
        base: Rope,
        source: Rope,
    },
    Binary,
}

/// The `Editor` field. Holds the stream, when there is one.
#[derive(Default)]
pub struct Review {
    stream: Option<Stream>,
    /// Whether a view may still have the stream's line number width.
    widened: bool,
}

impl Review {
    pub fn stream(&self) -> Option<&Stream> {
        self.stream.as_ref()
    }

    pub fn stream_of(&self, doc: DocumentId) -> Option<&Stream> {
        self.stream.as_ref().filter(|stream| stream.doc == doc)
    }
}

pub struct Stream {
    pub doc: DocumentId,
    pub files: Vec<FileChange>,
    /// One per line of the buffer.
    pub rows: Vec<Row>,
    /// What the buffer has to hold.
    text: Rope,
    /// The buffer's version when it last held `text`.
    version: i32,
    /// Each view's selection when the buffer last held `text`.
    selections: HashMap<ViewId, Selection>,
    /// Digits of the widest base and source line numbers.
    widths: [usize; 2],
}

/// Where `ret` on a row opens its file.
pub struct Target<'a> {
    pub change: &'a FileChange,
    /// The zero-based line of the source.
    pub line: usize,
    /// Whether the row shows that line, so the cursor's column carries over.
    pub same_line: bool,
}

impl Stream {
    pub fn gutter_width(&self) -> usize {
        self.widths[0] + 1 + self.widths[1]
    }

    pub fn target(&self, row: usize) -> Option<Target<'_>> {
        let (line, same_line) = match *self.rows.get(row)? {
            Row::Context { source, .. } | Row::Added { source, .. } => (source, true),
            Row::Gap { start, .. } => (start, false),
            Row::Removed { .. } => (self.line_below(row), false),
            Row::Header { file } => {
                let change = self.rows[row + 1..]
                    .iter()
                    .take_while(|next| next.file() == file)
                    .position(|next| matches!(next, Row::Removed { .. } | Row::Added { .. }));
                let line =
                    match change.map(|offset| (row + 1 + offset, self.rows[row + 1 + offset])) {
                        Some((_, Row::Added { source, .. })) => source,
                        Some((change, _)) => self.line_below(change),
                        None => 0,
                    };
                (line, false)
            }
        };
        Some(Target {
            change: &self.files[self.rows[row].file()],
            line,
            same_line,
        })
    }

    fn line_below(&self, row: usize) -> usize {
        let file = self.rows[row].file();
        let same_file = |other: &&Row| other.file() == file;
        let next = self.rows[row + 1..]
            .iter()
            .take_while(same_file)
            .find_map(|next| next.lines().1);
        let previous = || {
            self.rows[..row]
                .iter()
                .rev()
                .take_while(same_file)
                .find_map(|previous| previous.lines().1)
                .map(|line| line + 1)
        };
        next.or_else(previous).unwrap_or(0)
    }
}

struct Layout {
    files: Vec<FileChange>,
    rows: Vec<Row>,
    text: String,
    widths: [usize; 2],
}

fn layout(sections: Vec<Section>) -> Layout {
    let mut files = Vec::with_capacity(sections.len());
    let mut rows = Vec::new();
    let mut text = String::new();
    for (file, section) in sections.into_iter().enumerate() {
        let (body, note) = match &section.content {
            Content::Binary => (Vec::new(), Some("binary")),
            Content::Text { base, source } => match layout::hunks(base, source) {
                None => (Vec::new(), Some("too large to diff")),
                Some(hunks) if hunks.is_empty() => (Vec::new(), Some("no text changes")),
                Some(hunks) => (
                    layout::rows(file, &hunks, layout::line_count(source), layout::CONTEXT),
                    None,
                ),
            },
        };

        text.push_str(&header(&section.change, note));
        text.push('\n');
        rows.push(Row::Header { file });
        for row in &body {
            match (*row, &section.content) {
                (Row::Gap { start, end, .. }, _) => {
                    let lines = end - start;
                    let plural = if lines == 1 { "" } else { "s" };
                    text.push_str(&format!("⋯ {lines} unchanged line{plural}\n"));
                }
                (Row::Removed { base, .. }, Content::Text { base: rope, .. }) => {
                    push_line(&mut text, rope.line(base))
                }
                (
                    Row::Context { source, .. } | Row::Added { source, .. },
                    Content::Text { source: rope, .. },
                ) => push_line(&mut text, rope.line(source)),
                _ => unreachable!("only a text section has rows below its header"),
            }
        }
        rows.extend(body);
        files.push(section.change);
    }

    let digits = |line: Option<usize>| line.map_or(1, |line| (line + 1).to_string().len());
    let widths = [
        digits(rows.iter().filter_map(|row| row.lines().0).max()),
        digits(rows.iter().filter_map(|row| row.lines().1).max()),
    ];
    Layout {
        files,
        rows,
        text,
        widths,
    }
}

fn push_line(text: &mut String, line: RopeSlice) {
    // INVARIANT: a row keeps its line's own line ending, so the row's chars
    // match the line's one to one. `ret` carries the column over on that.
    text.extend(line.chunks());
    if get_line_ending(&line).is_none() {
        text.push('\n');
    }
}

fn header(change: &FileChange, note: Option<&str>) -> String {
    let show = |path: &Path| {
        helix_stdx::path::get_relative_path(path)
            .display()
            .to_string()
    };
    let status = match change {
        FileChange::Modified { .. } => "modified".to_string(),
        FileChange::Untracked { .. } | FileChange::Added { .. } => "new file".to_string(),
        FileChange::Deleted { .. } => "deleted".to_string(),
        FileChange::Conflict { .. } => "conflict".to_string(),
        FileChange::Renamed { from_path, .. } => format!("renamed from {}", show(from_path)),
    };
    match note {
        Some(note) => format!("{}  {status}, {note}", show(change.path())),
        None => format!("{}  {status}", show(change.path())),
    }
}

pub fn show(editor: &mut Editor, sections: Vec<Section>) {
    let layout = layout(sections);
    let open = editor
        .review
        .stream
        .as_ref()
        .map(|stream| stream.doc)
        .filter(|doc| editor.documents.contains_key(doc));
    let doc_id = match open {
        Some(doc) => {
            editor.switch(doc, Action::Replace);
            doc
        }
        None => editor.new_file(Action::Replace),
    };

    let view = editor.tree.focus;
    let doc = editor.documents.get_mut(&doc_id).expect("just opened");
    doc.readonly = true;
    let text = Rope::from(layout.text.as_str());
    let transaction = Transaction::change(
        doc.text(),
        [(0, doc.text().len_chars(), Some(layout.text.into()))].into_iter(),
    )
    .with_selection(Selection::point(0));
    doc.apply(&transaction, view);
    sync::settle(doc, &mut editor.tree, view);
    align_view(doc, editor.tree.get(view), Align::Top);
    let version = doc.version();

    editor.review.stream = Some(Stream {
        doc: doc_id,
        files: layout.files,
        rows: layout.rows,
        text,
        version,
        selections: HashMap::new(),
        widths: layout.widths,
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn modified(name: &str, base: &str, source: &str) -> Section {
        Section {
            change: FileChange::Modified {
                path: PathBuf::from(name),
            },
            content: Content::Text {
                base: Rope::from(base),
                source: Rope::from(source),
            },
        }
    }

    #[test]
    fn layout_text_has_one_line_per_row() {
        let layout = layout(vec![
            modified(
                "a.txt",
                "1\n2\n3\n4\n5\n6\n7\n8\n9\n",
                "1\n2\n3\n4\n5\nsix\n7\n8\n9\n",
            ),
            modified("b.txt", "x\r\ny", "x\r\nz"),
            Section {
                change: FileChange::Added {
                    path: PathBuf::from("c.bin"),
                },
                content: Content::Binary,
            },
        ]);
        assert_eq!(
            layout.text,
            "a.txt  modified\n\
             ⋯ 2 unchanged lines\n\
             3\n4\n5\n6\nsix\n7\n8\n9\n\
             b.txt  modified\n\
             x\r\ny\nz\n\
             c.bin  new file, binary\n"
        );
        assert_eq!(layout.rows.len(), Rope::from(layout.text).len_lines() - 1);
        assert_eq!(layout.widths, [1, 1]);
    }

    #[test]
    fn widths_fit_the_widest_line_number_on_each_side() {
        let base: String = (1..=1002).map(|n| format!("{n}\n")).collect();
        let source: String = (6..=1002)
            .map(|n| match n {
                1000 => "thousand\n".to_string(),
                n => format!("{n}\n"),
            })
            .collect();
        let shown = layout(vec![modified("a.txt", &base, &source)]);
        assert_eq!(shown.widths, [4, 3]);

        let shown = layout(vec![modified("a.txt", "", "new\n")]);
        assert_eq!(shown.widths, [1, 1]);
    }

    fn stream(sections: Vec<Section>) -> Stream {
        let layout = layout(sections);
        Stream {
            doc: DocumentId::default(),
            files: layout.files,
            rows: layout.rows,
            text: Rope::from(layout.text),
            version: 0,
            selections: HashMap::new(),
            widths: layout.widths,
        }
    }

    #[test]
    fn target_lands_on_the_source_line_a_row_stands_for() {
        let base = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let source = "A\nb\nc\nd\ne\nf\ng\nh\ni\n";
        let stream = stream(vec![
            modified("one.txt", base, source),
            Section {
                change: FileChange::Untracked {
                    path: PathBuf::from("two.txt"),
                },
                content: Content::Text {
                    base: Rope::new(),
                    source: Rope::from("new\n"),
                },
            },
        ]);
        let target = |row| {
            let target = stream.target(row).unwrap();
            (
                target.change.path().to_str().unwrap().to_string(),
                target.line,
                target.same_line,
            )
        };
        let one = |line, same_line| ("one.txt".to_string(), line, same_line);
        assert_eq!(target(0), one(0, false), "a header opens its first change");
        assert_eq!(
            target(1),
            one(0, false),
            "a removed row opens the line below it"
        );
        assert_eq!(target(2), one(0, true), "an added row opens its own line");
        assert_eq!(target(3), one(1, true), "a context row opens its own line");
        assert_eq!(
            target(6),
            one(4, false),
            "a gap opens its first hidden line"
        );
        assert_eq!(
            target(10),
            one(9, false),
            "a removal at the end opens past the last line"
        );
        assert_eq!(target(12), ("two.txt".to_string(), 0, false));
        assert!(stream.target(14).is_none());
    }
}
