//! FORK: inline diff. The words that changed inside a modified hunk.

use std::ops::Range;

use helix_core::RopeSlice;
use imara_diff::{Algorithm, Diff, InternedInput};

/// Hunks larger than this get no word highlights. A word diff of a big
/// rewrite is slow and marks nearly everything anyway.
const MAX_HUNK_BYTES: usize = 16 * 1024;

/// The changed words of one hunk, as char ranges into each text, in order.
/// A range never crosses a line or starts or ends with whitespace.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ChangedWords {
    pub base: Vec<Range<usize>>,
    pub doc: Vec<Range<usize>>,
}

/// Diffs the words of `base_lines` in `base` against `doc_lines` in `doc`.
///
/// Returns `None` when either side has no lines, as in a pure insertion or
/// removal, and when the hunk is too big.
pub fn changed_words(
    base: RopeSlice,
    base_lines: Range<usize>,
    doc: RopeSlice,
    doc_lines: Range<usize>,
) -> Option<ChangedWords> {
    let base = Side::new(base, base_lines)?;
    let doc = Side::new(doc, doc_lines)?;
    if base.text.len() + doc.text.len() > MAX_HUNK_BYTES {
        return None;
    }
    let base_tokens = tokens(&base.text);
    let doc_tokens = tokens(&doc.text);

    let mut input = InternedInput::default();
    input.update_before(base_tokens.iter().map(|token| token.text));
    input.update_after(doc_tokens.iter().map(|token| token.text));
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_no_heuristic(&input);

    let mut words = ChangedWords::default();
    for hunk in diff.hunks() {
        let range = |range: Range<u32>| range.start as usize..range.end as usize;
        base.push_changed(&base_tokens[range(hunk.before)], &mut words.base);
        doc.push_changed(&doc_tokens[range(hunk.after)], &mut words.doc);
    }
    Some(words)
}

/// One side of a hunk.
struct Side {
    text: String,
    /// The char index of `text` in its rope.
    start: usize,
}

impl Side {
    fn new(rope: RopeSlice, lines: Range<usize>) -> Option<Side> {
        // The diff can lag the buffer by a frame.
        let lines = lines.start.min(rope.len_lines())..lines.end.min(rope.len_lines());
        if lines.is_empty() {
            return None;
        }
        let start = rope.line_to_char(lines.start);
        let end = rope.line_to_char(lines.end);
        Some(Side {
            text: rope.slice(start..end).into(),
            start,
        })
    }

    /// Pushes the changed `tokens`, split at line ends and trimmed of whitespace.
    fn push_changed(&self, tokens: &[Token], out: &mut Vec<Range<usize>>) {
        for line in tokens.split(|token| token.kind == Kind::Newline) {
            let mut words = line.iter().filter(|token| token.kind != Kind::Space);
            if let Some(first) = words.next() {
                let last = words.next_back().unwrap_or(first);
                out.push(self.start + first.start..self.start + last.end);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Newline,
    Other,
}

impl Kind {
    fn of(c: char) -> Kind {
        match c {
            '\n' | '\r' => Kind::Newline,
            c if c.is_alphanumeric() || c == '_' => Kind::Word,
            c if c.is_whitespace() => Kind::Space,
            _ => Kind::Other,
        }
    }
}

struct Token<'a> {
    text: &'a str,
    kind: Kind,
    /// Char offsets into the side's text.
    start: usize,
    end: usize,
}

/// Splits `text` into runs of word chars, runs of spaces, and single other chars.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut tokens: Vec<Token> = Vec::new();
    for (chars, (byte, c)) in text.char_indices().enumerate() {
        let kind = Kind::of(c);
        match tokens.last_mut() {
            Some(token) if token.kind == kind && matches!(kind, Kind::Word | Kind::Space) => {
                token.text = &text[byte - token.text.len()..byte + c.len_utf8()];
                token.end += 1;
            }
            _ => tokens.push(Token {
                text: &text[byte..byte + c.len_utf8()],
                kind,
                start: chars,
                end: chars + 1,
            }),
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use helix_core::Rope;

    use super::*;

    /// The changed words of a hunk spanning all of `base` and `doc`, as text.
    fn words(base: &str, doc: &str) -> Option<(Vec<String>, Vec<String>)> {
        let (base, doc) = (Rope::from(base), Rope::from(doc));
        let words = changed_words(
            base.slice(..),
            0..base.len_lines(),
            doc.slice(..),
            0..doc.len_lines(),
        )?;
        let text = |rope: &Rope, ranges: Vec<Range<usize>>| {
            ranges
                .into_iter()
                .map(|range| rope.slice(range).to_string())
                .collect()
        };
        Some((text(&base, words.base), text(&doc, words.doc)))
    }

    #[test]
    fn marks_the_words_that_changed() {
        let (base, doc) = words(
            "    let width = area.width - 2;\n",
            "    let width = area.width.saturating_sub(2);\n",
        )
        .unwrap();
        assert_eq!(base, ["-"]);
        assert_eq!(doc, [".saturating_sub(", ")"]);
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn ranges_index_the_whole_rope() {
        let base = Rope::from("keep\nold value\n");
        let doc = Rope::from("keep\nnew value\n");
        let words = changed_words(base.slice(..), 1..2, doc.slice(..), 1..2).unwrap();
        assert_eq!(words.base, [5..8]);
        assert_eq!(words.doc, [5..8]);
    }

    #[test]
    fn splits_at_lines_and_trims_whitespace() {
        let (base, doc) = words("a b\nc d\n", "a x\n  y d\n").unwrap();
        // "b\nc" changed to "x\n  y". Each line gets its own range, without the indent.
        assert_eq!(base, ["b", "c"]);
        assert_eq!(doc, ["x", "y"]);
    }

    #[test]
    fn whitespace_changes_mark_nothing() {
        assert_eq!(words("  x = 1\n", "    x  =  1\n"), Some((vec![], vec![])));
    }

    #[test]
    fn multibyte_text_uses_char_offsets() {
        let (base, doc) = words("café = 1\n", "café = 2\n").unwrap();
        assert_eq!(base, ["1"]);
        assert_eq!(doc, ["2"]);
    }

    #[test]
    fn needs_both_sides() {
        let base = Rope::from("gone\n");
        let doc = Rope::from("kept\n");
        assert_eq!(
            changed_words(base.slice(..), 0..1, doc.slice(..), 0..0),
            None
        );
    }

    #[test]
    fn skips_huge_hunks() {
        let base = "a ".repeat(MAX_HUNK_BYTES / 2);
        assert_eq!(words(&base, "b\n"), None);
    }

    #[test]
    fn clamps_a_lagging_diff() {
        let base = Rope::from("a\nb\n");
        let doc = Rope::from("a\n");
        assert_eq!(
            changed_words(base.slice(..), 1..2, doc.slice(..), 3..5),
            None
        );
    }
}
