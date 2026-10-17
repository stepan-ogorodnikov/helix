//! The review stream: one read-only scratch buffer that shows the hunks of
//! every changed file, one row per line.
//!
//! `helix-term` lists the changed files and reads both sides of each. This
//! module lays out the rows, owns the buffer's text, and keeps it read-only.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use helix_core::Language;
use helix_core::{line_ending::get_line_ending, Rope, RopeSlice, Selection};
use helix_vcs::FileChange;

use crate::editor::Action;
use crate::{align_view, view, Align, DocumentId, Editor, ViewId};
use reconcile::Key;

pub(crate) mod gutter;
pub mod layout;
mod reconcile;
mod sync;
mod syntax;
mod words;

pub use sync::sync;
pub use syntax::overlays as syntax_overlays;

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

    fn key(self) -> (usize, Key) {
        match self {
            Row::Header { file } => (file, Key::Header),
            Row::Gap { file, start, .. } => (file, Key::Gap(start)),
            Row::Removed { file, base } => (file, Key::Removed(base)),
            Row::Context { file, source, .. } | Row::Added { file, source } => {
                (file, Key::Source(source))
            }
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

#[derive(Clone)]
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
    /// The directory the changes were listed from.
    pub root: PathBuf,
    files: Vec<File>,
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
    pub ranges: Ranges,
}

/// The rows of each hunk: a run of removed and added rows.
fn hunks(rows: &[Row]) -> Vec<Range<usize>> {
    let mut hunks: Vec<Range<usize>> = Vec::new();
    for (row, kind) in rows.iter().enumerate() {
        if !matches!(kind, Row::Removed { .. } | Row::Added { .. }) {
            continue;
        }
        match hunks.last_mut() {
            Some(hunk) if hunk.end == row => hunk.end += 1,
            _ => hunks.push(row..row + 1),
        }
    }
    hunks
}

/// Char ranges of the stream's text, by how they're drawn. Each list is in
/// order, and leaves out line endings.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ranges {
    pub removed: Vec<Range<usize>>,
    pub headers: Vec<Range<usize>>,
    pub gaps: Vec<Range<usize>>,
    /// The words that changed, on both sides of a hunk with removed and added
    /// rows.
    pub words: Vec<Range<usize>>,
}

impl Ranges {
    fn new(rows: &[Row], hunks: &[Range<usize>], text: &Rope) -> Ranges {
        let mut ranges = Ranges::default();
        for (row, kind) in rows.iter().enumerate() {
            let list = match kind {
                Row::Removed { .. } => &mut ranges.removed,
                Row::Header { .. } => &mut ranges.headers,
                Row::Gap { .. } => &mut ranges.gaps,
                Row::Context { .. } | Row::Added { .. } => continue,
            };
            let line = text.line(row);
            let ending = get_line_ending(&line).map_or(0, |ending| ending.len_chars());
            let start = text.line_to_char(row);
            list.push(start..start + line.len_chars() - ending);
        }
        let text = text.slice(..);
        for hunk in hunks {
            let added = hunk.start
                + rows[hunk.clone()]
                    .iter()
                    .take_while(|row| matches!(row, Row::Removed { .. }))
                    .count();
            if let Some(words) =
                words::changed_words(text, hunk.start..added, text, added..hunk.end)
            {
                ranges.words.extend(words.base);
                ranges.words.extend(words.doc);
            }
        }
        ranges
    }
}

/// A file of the stream, and what its rows came from.
#[derive(Clone)]
struct File {
    change: FileChange,
    content: Content,
    /// The open buffer the source came from, and its version then.
    seen: Option<(DocumentId, i32)>,
    /// Source lines whose gap was opened.
    opened: Vec<Range<usize>>,
    language: Option<Language>,
    trees: syntax::Trees,
}

impl From<Section> for File {
    fn from(section: Section) -> File {
        File {
            change: section.change,
            content: section.content,
            seen: None,
            opened: Vec::new(),
            language: None,
            trees: syntax::Trees::default(),
        }
    }
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
    pub fn hunks(&self) -> Vec<Range<usize>> {
        hunks(&self.rows)
    }

    pub fn headers(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, Row::Header { .. }))
            .map(|(row, _)| row)
    }

    pub fn gutter_width(&self) -> usize {
        self.widths[0] + 1 + self.widths[1]
    }

    pub fn target(&self, row: usize) -> Option<Target<'_>> {
        let (line, same_line) = match *self.rows.get(row)? {
            Row::Context { source, .. } | Row::Added { source, .. } => (source, true),
            Row::Gap { .. } => return None,
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
            change: &self.files[self.rows[row].file()].change,
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
    rows: Vec<Row>,
    text: String,
    widths: [usize; 2],
}

fn layout(files: &[File]) -> Layout {
    let mut rows = Vec::new();
    let mut text = String::new();
    for (index, file) in files.iter().enumerate() {
        let (body, note) = match &file.content {
            Content::Binary => (Vec::new(), Some("binary")),
            Content::Text { base, source } => match layout::hunks(base, source) {
                None => (Vec::new(), Some("too large to diff")),
                Some(hunks) if hunks.is_empty() => (Vec::new(), Some("no text changes")),
                Some(hunks) => {
                    let lines = layout::line_count(source);
                    let rows = layout::rows(index, &hunks, lines, layout::CONTEXT, &file.opened);
                    (rows, None)
                }
            },
        };

        text.push_str(&header(&file.change, note));
        text.push('\n');
        rows.push(Row::Header { file: index });
        for row in &body {
            match (*row, &file.content) {
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
                _ => unreachable!("only a text file has rows below its header"),
            }
        }
        rows.extend(body);
    }

    let digits = |line: Option<usize>| line.map_or(1, |line| (line + 1).to_string().len());
    let widths = [
        digits(rows.iter().filter_map(|row| row.lines().0).max()),
        digits(rows.iter().filter_map(|row| row.lines().1).max()),
    ];
    Layout { rows, text, widths }
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

pub fn show(editor: &mut Editor, root: PathBuf, sections: Vec<Section>) {
    let open = editor
        .review
        .stream
        .as_ref()
        .map(|stream| stream.doc)
        .filter(|doc| editor.documents.contains_key(doc));
    if let Some(doc) = open {
        editor.switch(doc, Action::Replace);
        if let Some(stream) = editor.review.stream.as_mut() {
            stream.root = root;
        }
        update(editor, sections.into_iter().map(File::from).collect(), None);
        return;
    }

    let doc_id = editor.new_file(Action::Replace);
    let doc = editor.documents.get_mut(&doc_id).expect("just opened");
    doc.readonly = true;
    editor.review.stream = Some(Stream {
        doc: doc_id,
        root,
        files: Vec::new(),
        rows: Vec::new(),
        text: doc.text().clone(),
        version: doc.version(),
        selections: HashMap::new(),
        widths: [1, 1],
        ranges: Ranges::default(),
    });
    update(editor, sections.into_iter().map(File::from).collect(), None);
    let view = editor.tree.get(editor.tree.focus);
    let doc = editor.documents.get_mut(&doc_id).expect("just opened");
    doc.set_selection(view.id, Selection::point(0));
    align_view(doc, view, Align::Top);
}

pub fn open_gap(editor: &mut Editor, row: usize) -> bool {
    let Some(stream) = editor.review.stream_of(view!(editor).doc) else {
        return false;
    };
    let Some(&Row::Gap { file, start, end }) = stream.rows.get(row) else {
        return false;
    };
    let files = stream.files.clone();
    update(editor, files, Some((file, start..end)));
    true
}

// INVARIANT: `open` is a range of `files`' source lines, not the stream's.
fn update(editor: &mut Editor, mut files: Vec<File>, open: Option<(usize, Range<usize>)>) {
    let Some(doc_id) = editor.review.stream.as_ref().map(|stream| stream.doc) else {
        return;
    };
    // INVARIANT: the transaction below is built on the stream's text, so the
    // buffer has to hold it.
    if sync::put_back(editor, doc_id) {
        sync::refused(editor);
    }
    let focus = editor.tree.focus;
    let shown_in = std::iter::once(editor.tree.get(focus))
        .chain(editor.tree.views().map(|(view, _)| view))
        .find(|view| view.doc == doc_id)
        .map(|view| view.id);
    // INVARIANT: the buffer's history is committed through a view of it.
    let Some(view) = shown_in else {
        return;
    };
    let loader = editor.syn_loader.load();
    for file in &mut files {
        file.seen = editor
            .document_by_path(file.change.path())
            .map(|doc| (doc.id(), doc.version()));
        if let Content::Text { source, .. } = &file.content {
            file.language = loader
                .language_for_filename(file.change.path())
                .or_else(|| loader.language_for_shebang(source.slice(..)));
        }
    }
    drop(loader);

    let Some(stream) = editor.review.stream.as_ref() else {
        return;
    };
    let index: HashMap<PathBuf, usize> = files
        .iter()
        .enumerate()
        .map(|(new, file)| (file.change.path().to_path_buf(), new))
        .collect();
    let mut maps = Vec::with_capacity(stream.files.len());
    for old in &stream.files {
        let Some(&new) = index.get(old.change.path()) else {
            maps.push(None);
            continue;
        };
        let (base, source) = match (&old.content, &files[new].content) {
            (
                Content::Text {
                    base: old_base,
                    source: old_source,
                },
                Content::Text { base, source },
            ) => (
                layout::line_map(old_base, base),
                layout::line_map(old_source, source),
            ),
            // INVARIANT: never a side kept as unchanged. A binary file has no
            // parses to keep.
            _ => (Some(Vec::new()), Some(Vec::new())),
        };
        files[new].opened = layout::map_ranges(&old.opened, source.as_deref());
        // WHY: a side whose text didn't change keeps its parse.
        if base.is_none() {
            files[new].trees.base = old.trees.base.clone();
        }
        if source.is_none() {
            files[new].trees.source = old.trees.source.clone();
        }
        maps.push(Some((new, base, source)));
    }
    if let Some((file, lines)) = open {
        files[file].opened.push(lines);
    }

    let layout = layout(&files);
    let old_keys: Vec<_> = stream
        .rows
        .iter()
        .map(|row| {
            let (file, key) = row.key();
            let (new, base, source) = maps[file].as_ref()?;
            let key = match key {
                Key::Header => Key::Header,
                Key::Gap(line) => Key::Gap(layout::map_line(source.as_deref(), line)?),
                Key::Removed(line) => Key::Removed(layout::map_line(base.as_deref(), line)?),
                Key::Source(line) => Key::Source(layout::map_line(source.as_deref(), line)?),
            };
            Some((*new, key))
        })
        .collect();
    let new_keys: Vec<_> = layout.rows.iter().map(|row| row.key()).collect();
    let text = Rope::from(layout.text.as_str());
    let transaction = reconcile::transaction(&stream.text, &old_keys, &text, &new_keys);

    let doc = editor.documents.get_mut(&doc_id).expect("a view shows it");
    doc.apply(&transaction, view);
    sync::settle(doc, &mut editor.tree, view);
    debug_assert_eq!(*doc.text(), text);
    let version = doc.version();
    let stream = editor.review.stream.as_mut().expect("checked above");
    stream.ranges = Ranges::new(&layout.rows, &hunks(&layout.rows), &text);
    stream.files = files;
    stream.rows = layout.rows;
    stream.text = text;
    stream.version = version;
    stream.widths = layout.widths;
    sync::save_selections(editor, doc_id);
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
        let layout = layout(&files(vec![
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
        ]));
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
        let shown = layout(&files(vec![modified("a.txt", &base, &source)]));
        assert_eq!(shown.widths, [4, 3]);

        let shown = layout(&files(vec![modified("a.txt", "", "new\n")]));
        assert_eq!(shown.widths, [1, 1]);
    }

    fn files(sections: Vec<Section>) -> Vec<File> {
        sections.into_iter().map(File::from).collect()
    }

    fn stream(sections: Vec<Section>) -> Stream {
        stream_of(files(sections))
    }

    pub(super) fn stream_of(files: Vec<File>) -> Stream {
        let layout = layout(&files);
        Stream {
            doc: DocumentId::default(),
            root: PathBuf::new(),
            files,
            rows: layout.rows,
            text: Rope::from(layout.text.as_str()),
            version: 0,
            selections: HashMap::new(),
            widths: layout.widths,
            ranges: Ranges::default(),
        }
    }

    #[test]
    fn hunks_are_runs_of_changed_rows_within_a_file() {
        let base = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let source = "A\nb\nc\nd\ne\nf\ng\nh\ni\n";
        let stream = stream(vec![
            modified("one.txt", base, source),
            modified("two.txt", "x\n", "y\n"),
        ]);
        assert_eq!(stream.hunks(), [1..3, 10..12, 13..15]);
        assert_eq!(stream.headers().collect::<Vec<_>>(), [0, 12]);
    }

    #[test]
    fn ranges_cover_rows_without_their_line_endings_and_the_changed_words() {
        let shown = layout(&files(vec![modified(
            "a.txt",
            "x = 1\r\nkeep\n",
            "x = 2\r\nkeep\n",
        )]));
        let text = Rope::from(shown.text.as_str());
        let ranges = Ranges::new(&shown.rows, &hunks(&shown.rows), &text);
        let texts = |ranges: &[Range<usize>]| -> Vec<String> {
            ranges
                .iter()
                .map(|range| text.slice(range.clone()).to_string())
                .collect()
        };
        assert_eq!(text, "a.txt  modified\nx = 1\r\nx = 2\r\nkeep\n");
        assert_eq!(texts(&ranges.headers), ["a.txt  modified"]);
        assert_eq!(texts(&ranges.removed), ["x = 1"]);
        assert!(ranges.gaps.is_empty());
        assert_eq!(texts(&ranges.words), ["1", "2"]);
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
        assert!(
            stream.target(6).is_none(),
            "a gap opens in place, not in the file"
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
