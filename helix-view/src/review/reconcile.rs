//! Turns the rows the stream has into the rows it should have, with one
//! transaction that keeps every row that still stands for the same line.
//! Selections map through it like through any edit, so the cursor stays on its
//! line while rows come and go around it.

use std::collections::HashMap;

use helix_core::{line_ending::get_line_ending, Rope, Transaction};

/// What a row stands for within its file. Rows match by this, never by text:
/// a removed `}` next to a kept `}` has the same text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Key {
    Header,
    /// A gap, by its first hidden source line.
    Gap(usize),
    Removed(usize),
    /// A context or added row, by its source line. A context row that turns
    /// into an added row keeps its key.
    Source(usize),
}

// INVARIANT: `old_keys` are in the new stream's terms: its file indices and
// line numbers. An old row with no key goes away.
pub(super) fn transaction(
    old: &Rope,
    old_keys: &[Option<(usize, Key)>],
    new: &Rope,
    new_keys: &[(usize, Key)],
) -> Transaction {
    let wanted: HashMap<_, _> = new_keys
        .iter()
        .enumerate()
        .map(|(row, key)| (*key, row))
        .collect();
    // INVARIANT: kept rows stay in order. A row matched out of order is
    // replaced instead, so the changes below never overlap.
    let mut kept = Vec::new();
    for (row, key) in old_keys.iter().enumerate() {
        let Some(&new_row) = key.as_ref().and_then(|key| wanted.get(key)) else {
            continue;
        };
        if kept.last().is_none_or(|&(_, last)| new_row > last) {
            kept.push((row, new_row));
        }
    }

    // WHY: the last change runs to the end of the text, not of the last row.
    // A new scratch buffer holds a line ending and no rows.
    let old_end = |row| match row == old_keys.len() {
        true => old.len_chars(),
        false => old.line_to_char(row),
    };
    let mut changes = Vec::new();
    let (mut old_row, mut new_row) = (0, 0);
    let ends = (old_keys.len(), new_keys.len());
    for (old_kept, new_kept) in kept.into_iter().chain(std::iter::once(ends)) {
        let (from, to) = (old.line_to_char(old_row), old_end(old_kept));
        if from < to || new_row < new_kept {
            let inserted = new.slice(new.line_to_char(new_row)..new.line_to_char(new_kept));
            changes.push((from, to, Some(inserted.to_string().into())));
        }
        if (old_kept, new_kept) != ends {
            let (from, to) = (old.line(old_kept), new.line(new_kept));
            if from != to {
                let start = old.line_to_char(old_kept);
                // WHY: only the text before the line ending, when both have the
                // same one. A cursor on the row then stays on it.
                let ending = match (get_line_ending(&from), get_line_ending(&to)) {
                    (Some(a), Some(b)) if a == b => a.len_chars(),
                    _ => 0,
                };
                let text = to.slice(..to.len_chars() - ending).to_string();
                changes.push((start, start + from.len_chars() - ending, Some(text.into())));
            }
        }
        (old_row, new_row) = (old_kept + 1, new_kept + 1);
    }
    Transaction::change(old, changes.into_iter())
}

#[cfg(test)]
mod tests {
    use helix_core::Selection;

    use super::*;

    /// Applies the transaction from `old` to `new` with the cursor on row
    /// `cursor`, and returns the row the cursor lands on.
    fn land(
        old: &str,
        old_keys: &[Option<(usize, Key)>],
        new: &str,
        new_keys: &[(usize, Key)],
        cursor: usize,
    ) -> usize {
        let (old, new) = (Rope::from(old), Rope::from(new));
        let transaction = transaction(&old, old_keys, &new, new_keys);
        let mut text = old.clone();
        assert!(transaction.apply(&mut text));
        assert_eq!(text, new);
        let start = old.line_to_char(cursor);
        let selection = Selection::single(start, start + 1).map(transaction.changes());
        text.char_to_line(selection.primary().cursor(text.slice(..)))
    }

    use Key::*;

    #[test]
    fn rows_that_stand_for_the_same_line_keep_the_cursor() {
        let header = Some((0, Header));
        assert_eq!(
            land(
                "a.txt\n4\n",
                &[header, Some((0, Source(3)))],
                "a.txt\n4\nfour\n",
                &[(0, Header), (0, Removed(3)), (0, Source(3))],
                1,
            ),
            2,
            "a context row that turns into an added row keeps the cursor, below its new removed twin"
        );
        assert_eq!(
            land(
                "a.txt\n3\nthree\n4\n",
                &[
                    header,
                    Some((0, Removed(2))),
                    Some((0, Source(2))),
                    Some((0, Source(3)))
                ],
                "a.txt\n3\n4\n",
                &[(0, Header), (0, Source(2)), (0, Source(3))],
                3,
            ),
            2,
            "a removed twin vanishes from above the cursor"
        );
        assert_eq!(
            land(
                "a.txt\n⋯ 2 unchanged lines\n3\nb.txt\nb\n",
                &[
                    header,
                    Some((0, Gap(0))),
                    Some((0, Source(2))),
                    Some((1, Header)),
                    Some((1, Source(0)))
                ],
                "a.txt\n1\n⋯ 1 unchanged line\n3\nb.txt\nb\n",
                &[
                    (0, Header),
                    (0, Source(0)),
                    (0, Gap(1)),
                    (0, Source(2)),
                    (1, Header),
                    (1, Source(0))
                ],
                4,
            ),
            5,
            "rows of another file stay as they are"
        );
    }

    #[test]
    fn a_row_with_no_key_goes_and_the_cursor_lands_on_what_replaced_it() {
        assert_eq!(
            land(
                "a.txt\nold\nkept\n",
                &[Some((0, Header)), None, Some((0, Source(1)))],
                "a.txt\nnew\nkept\n",
                &[(0, Header), (0, Source(0)), (0, Source(1))],
                1,
            ),
            1
        );
    }

    #[test]
    fn a_new_scratch_buffer_loses_its_line_ending() {
        assert_eq!(land("\n", &[], "a.txt\n", &[(0, Header)], 0), 0);
    }
}
