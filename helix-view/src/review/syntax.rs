//! Syntax highlights for the stream's rows, from parses of each file's whole
//! base and source. A hunk that starts inside a string still highlights right.
//!
//! A side is parsed off the main thread the first time its rows show, and kept
//! until its text changes. An open buffer that matches the source lends its own
//! tree.

use std::ops::Range;
use std::sync::Arc;

use helix_core::syntax::{Highlight, HighlightEvent, Loader, OverlayHighlights};
use helix_core::Language;
use helix_core::{Rope, RopeSlice, Syntax};
use helix_stdx::rope::RopeSliceExt;
use parking_lot::Mutex;

use super::{Content, Row, Stream};
use crate::Editor;

/// The syntax tree of one side of a file.
#[derive(Clone, Default)]
pub(super) struct Tree(Arc<Mutex<State>>);

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Parsing,
    Done(Option<Arc<Syntax>>),
}

impl Tree {
    /// The tree of `text`, or `None` while it parses or if it can't. The first
    /// call starts the parse, which requests a redraw when it's done.
    fn get(&self, text: &Rope, language: Language, loader: &Arc<Loader>) -> Option<Arc<Syntax>> {
        {
            let mut state = self.0.lock();
            match &*state {
                State::Done(syntax) => return syntax.clone(),
                State::Parsing => return None,
                State::Idle => *state = State::Parsing,
            }
        }
        let (tree, text, loader) = (self.clone(), text.clone(), loader.clone());
        let parse = move || {
            let syntax = Syntax::new(text.slice(..), language, &loader)
                .inspect_err(|err| log::info!("highlighting a review file failed: {err}"))
                .ok()
                .map(Arc::new);
            *tree.0.lock() = State::Done(syntax);
            helix_event::request_redraw();
        };
        // WORKAROUND: unit tests have no runtime, so they parse in place.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(parse)),
            Err(_) => parse(),
        }
        match &*self.0.lock() {
            State::Done(syntax) => syntax.clone(),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct Trees {
    pub base: Tree,
    pub source: Tree,
}

/// Overlays with the syntax highlights of `rows`, one per nesting depth so
/// that nested scopes patch in order. Removed rows are left out unless `base`.
pub fn overlays(
    editor: &Editor,
    stream: &Stream,
    rows: Range<usize>,
    base: bool,
) -> Vec<OverlayHighlights> {
    let loader = editor.syn_loader.load_full();
    let tree = |file: usize, removed: bool, with: &mut dyn FnMut(&Syntax, &Rope)| {
        let file = &stream.files[file];
        let Content::Text {
            base: base_text,
            source,
        } = &file.content
        else {
            return;
        };
        if !removed {
            // WHY: an open buffer's tree is current while its version is the
            // one the rows came from.
            let doc = editor.document_by_path(file.change.path());
            if let Some(doc) = doc.filter(|doc| Some((doc.id(), doc.version())) == file.seen) {
                if let Some(syntax) = doc.syntax() {
                    return with(syntax, doc.text());
                }
            }
        }
        let Some(language) = file.language else {
            return;
        };
        let (tree, text) = match removed {
            true => (&file.trees.base, base_text),
            false => (&file.trees.source, source),
        };
        if let Some(syntax) = tree.get(text, language, &loader) {
            with(&syntax, text);
        }
    };
    layers(stream, rows, base, &loader, tree)
}

fn layers(
    stream: &Stream,
    rows: Range<usize>,
    base: bool,
    loader: &Loader,
    tree: impl Fn(usize, bool, &mut dyn FnMut(&Syntax, &Rope)),
) -> Vec<OverlayHighlights> {
    let mut layers: Vec<Vec<(Highlight, Range<usize>)>> = Vec::new();
    let end = rows.end.min(stream.rows.len());
    let mut row = rows.start;
    while row < end {
        let (file, removed, line) = match stream.rows[row] {
            Row::Removed { file, base } => (file, true, base),
            Row::Context { file, source, .. } | Row::Added { file, source } => {
                (file, false, source)
            }
            Row::Header { .. } | Row::Gap { .. } => {
                row += 1;
                continue;
            }
        };
        // WHY: one highlighter per run of rows that show consecutive lines of
        // one side of a file, not one per row.
        let mut run_end = row + 1;
        while run_end < end {
            let next = match stream.rows[run_end] {
                Row::Removed { file, base } => (file, true, base),
                Row::Context { file, source, .. } | Row::Added { file, source } => {
                    (file, false, source)
                }
                _ => break,
            };
            if next != (file, removed, line + run_end - row) {
                break;
            }
            run_end += 1;
        }
        let first_row = row;
        row = run_end;
        if removed && !base {
            continue;
        }

        let lines = line..line + (run_end - first_row);
        tree(file, removed, &mut |syntax, text| {
            let text = text.slice(..);
            if lines.end > text.len_lines() {
                return;
            }
            let to_stream = |pos: usize| {
                let line = text.char_to_line(pos);
                let row = first_row + (line - lines.start);
                stream.text.line_to_char(row) + (pos - text.line_to_char(line))
            };
            spans(
                syntax,
                loader,
                text,
                lines.clone(),
                |depth, highlight, range| {
                    if layers.len() <= depth {
                        layers.resize_with(depth + 1, Vec::new);
                    }
                    let range = to_stream(range.start)..to_stream(range.end);
                    match layers[depth].last_mut() {
                        Some((last, last_range))
                            if *last == highlight && last_range.end == range.start =>
                        {
                            last_range.end = range.end
                        }
                        _ => layers[depth].push((highlight, range)),
                    }
                },
            );
        });
    }
    layers
        .into_iter()
        .map(|highlights| OverlayHighlights::Heterogenous { highlights })
        .collect()
}

/// Calls `push` with each highlight of `lines`, its nesting depth, and its
/// char range in `text`.
fn spans(
    syntax: &Syntax,
    loader: &Loader,
    text: RopeSlice,
    lines: Range<usize>,
    mut push: impl FnMut(usize, Highlight, Range<usize>),
) {
    let to_char = |byte: u32| text.byte_to_char(text.ceil_char_boundary(byte as usize));
    let start = text.line_to_byte(lines.start) as u32;
    let end = text.line_to_byte(lines.end) as u32;
    let mut highlighter = syntax.highlighter(text, loader, start..end);
    let mut stack = Vec::new();
    let mut pos = start;
    loop {
        let next = highlighter.next_event_offset().min(end);
        if next > pos {
            for (depth, &highlight) in stack.iter().enumerate() {
                push(depth, highlight, to_char(pos)..to_char(next));
            }
        }
        if next >= end {
            return;
        }
        pos = pos.max(next);
        let (event, highlights) = highlighter.advance();
        if let HighlightEvent::Refresh = event {
            stack.clear();
        }
        stack.extend(highlights);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use helix_vcs::FileChange;

    use super::*;
    use crate::review::{File, Section};
    use crate::Theme;

    #[test]
    fn highlights_land_on_the_rows_of_their_lines() {
        let theme: toml::Value = toml::from_str("keyword = \"red\"\nstring = \"green\"").unwrap();
        let theme = Theme::from(theme);
        let loader = Arc::new(helix_core::config::default_lang_loader());
        loader.set_scopes(theme.scopes().to_vec());
        let language = loader.language_for_name("rust").unwrap();
        let keyword = theme.find_highlight_exact("keyword").unwrap();
        let string = theme.find_highlight_exact("string").unwrap();

        // WHY: the hunk starts inside a raw string, so only a parse of the
        // whole source knows that `let` on its first row is string text.
        let base = "let s = r\"\n1\n2\n3\n4\n\";\nlet a = 1;\n";
        let source = "let s = r\"\nlet\n2\n3\n4\n\";\nlet a = 2;\n";
        let mut file = File::from(Section {
            change: FileChange::Modified {
                path: PathBuf::from("a.rs"),
            },
            content: Content::Text {
                base: Rope::from(base),
                source: Rope::from(source),
            },
        });
        file.language = Some(language);
        let stream = crate::review::tests::stream_of(vec![file]);
        let text = stream.text.to_string();
        assert_eq!(
            text,
            "a.rs  modified\nlet s = r\"\n1\nlet\n2\n3\n4\n\";\nlet a = 1;\nlet a = 2;\n"
        );

        let overlays = layers(
            &stream,
            0..stream.rows.len(),
            true,
            &loader,
            |file, removed, with| {
                let file = &stream.files[file];
                let Content::Text { base, source } = &file.content else {
                    return;
                };
                let (tree, text) = match removed {
                    true => (&file.trees.base, base),
                    false => (&file.trees.source, source),
                };
                with(&tree.get(text, language, &loader).unwrap(), text);
            },
        );
        let at = |highlight: Highlight| -> Vec<String> {
            let mut found = Vec::new();
            for overlay in &overlays {
                let OverlayHighlights::Heterogenous { highlights } = overlay else {
                    continue;
                };
                for (h, range) in highlights {
                    if *h == highlight {
                        found.push(text.chars().skip(range.start).take(range.len()).collect());
                    }
                }
            }
            found
        };
        let strings = at(string);
        assert!(
            strings.iter().any(|s| s.contains("let\n2")),
            "the row inside the string highlights as string: {strings:?}"
        );
        let keywords = at(keyword);
        assert_eq!(keywords, ["let", "let", "let"], "{keywords:?}");
    }
}
