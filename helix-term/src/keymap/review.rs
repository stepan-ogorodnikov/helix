//! The review stream's keymap: the user's keymap with the stream's bindings
//! over it. `EditorView` swaps it in while the stream has focus, so pending
//! keys and sticky modes stay with the keymap they started in.

use std::collections::HashMap;

use arc_swap::access::Constant;
use helix_core::hashmap;
use helix_view::{document::Mode, view, Editor};

use super::macros::keymap;
use super::{merge_keys, KeyTrie, Keymaps, MappableCommand};

#[derive(Default)]
pub struct ReviewKeys {
    /// The user's keymap, while the stream's is in `EditorView::keymaps`.
    user: Option<Keymaps>,
}

impl ReviewKeys {
    pub fn swap(&mut self, keymaps: &mut Keymaps, editor: &Editor) {
        let in_stream = editor.review.stream_of(view!(editor).doc).is_some();
        match (in_stream, self.user.take()) {
            (true, None) => {
                let mut map = keymaps.map().clone();
                for keys in map.values_mut() {
                    replace_commands(keys);
                }
                merge_keys(&mut map, bindings());
                let stream = Keymaps::new(Box::new(Constant(map)));
                self.user = Some(std::mem::replace(keymaps, stream));
            }
            (false, Some(user)) => *keymaps = user,
            (_, user) => self.user = user,
        }
    }
}

fn replace_commands(keys: &mut KeyTrie) {
    let review = |command: &MappableCommand| match command.name() {
        "goto_next_change" => Some(MappableCommand::review_next_hunk),
        "goto_prev_change" => Some(MappableCommand::review_prev_hunk),
        "goto_first_change" => Some(MappableCommand::review_first_hunk),
        "goto_last_change" => Some(MappableCommand::review_last_hunk),
        _ => None,
    };
    match keys {
        KeyTrie::MappableCommand(command) => {
            if let Some(review) = review(command) {
                *command = review;
            }
        }
        KeyTrie::Sequence(commands) => {
            for command in commands {
                if let Some(review) = review(command) {
                    *command = review;
                }
            }
        }
        KeyTrie::Node(node) => {
            for keys in node.values_mut() {
                replace_commands(keys);
            }
        }
    }
}

fn bindings() -> HashMap<Mode, KeyTrie> {
    let keys = keymap!({ "Review"
        "ret" => review_open,
        "[" => { "Left bracket"
            "F" => review_prev_file,
        },
        "]" => { "Right bracket"
            "F" => review_next_file,
        },
    });
    hashmap!(
        Mode::Normal => keys.clone(),
        Mode::Select => keys,
    )
}
