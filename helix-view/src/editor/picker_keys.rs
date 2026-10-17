use std::collections::HashMap;

use crate::input::KeyEvent;
use crate::keyboard::{KeyCode, KeyModifiers};

/// What a key does inside a picker. `Nop` removes a default binding so the
/// key is handled by the picker prompt instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerCommand {
    Previous,
    Next,
    PageUp,
    PageDown,
    First,
    Last,
    Open,
    OpenBackground,
    OpenHorizontal,
    OpenVertical,
    TogglePreview,
    Close,
    /// Cycle the changed-file picker: unstaged, staged, then both.
    /// Other pickers pass this key through to the prompt.
    CycleChanges,
    /// Jump from the changed-file picker into the review, at the selected
    /// file. Other pickers pass this key through to the prompt.
    ReviewJump,
    Nop,
}

impl PickerCommand {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "previous" => Self::Previous,
            "next" => Self::Next,
            "page_up" => Self::PageUp,
            "page_down" => Self::PageDown,
            "first" => Self::First,
            "last" => Self::Last,
            "open" => Self::Open,
            "open_background" => Self::OpenBackground,
            "open_horizontal" => Self::OpenHorizontal,
            "open_vertical" => Self::OpenVertical,
            "toggle_preview" => Self::TogglePreview,
            "close" => Self::Close,
            "cycle_changes" => Self::CycleChanges,
            "review_jump" => Self::ReviewJump,
            "nop" => Self::Nop,
            _ => return None,
        })
    }
}

pub fn default_picker_keys() -> HashMap<KeyEvent, PickerCommand> {
    let mut keys = HashMap::new();
    let bind = |keys: &mut HashMap<KeyEvent, PickerCommand>,
                code: KeyCode,
                modifiers: KeyModifiers,
                command: PickerCommand| {
        keys.insert(KeyEvent { code, modifiers }, command);
    };

    let none = KeyModifiers::NONE;
    let ctrl = KeyModifiers::CONTROL;
    let shift = KeyModifiers::SHIFT;
    let alt = KeyModifiers::ALT;

    bind(&mut keys, KeyCode::Tab, shift, PickerCommand::Previous);
    bind(&mut keys, KeyCode::Up, none, PickerCommand::Previous);
    bind(&mut keys, KeyCode::Char('p'), ctrl, PickerCommand::Previous);
    bind(&mut keys, KeyCode::Tab, none, PickerCommand::Next);
    bind(&mut keys, KeyCode::Down, none, PickerCommand::Next);
    bind(&mut keys, KeyCode::Char('n'), ctrl, PickerCommand::Next);
    bind(&mut keys, KeyCode::PageDown, none, PickerCommand::PageDown);
    bind(&mut keys, KeyCode::Char('d'), ctrl, PickerCommand::PageDown);
    bind(&mut keys, KeyCode::PageUp, none, PickerCommand::PageUp);
    bind(&mut keys, KeyCode::Char('u'), ctrl, PickerCommand::PageUp);
    bind(&mut keys, KeyCode::Home, none, PickerCommand::First);
    bind(&mut keys, KeyCode::End, none, PickerCommand::Last);
    bind(&mut keys, KeyCode::Enter, none, PickerCommand::Open);
    bind(
        &mut keys,
        KeyCode::Enter,
        alt,
        PickerCommand::OpenBackground,
    );
    bind(
        &mut keys,
        KeyCode::Char('s'),
        ctrl,
        PickerCommand::OpenHorizontal,
    );
    bind(
        &mut keys,
        KeyCode::Char('v'),
        ctrl,
        PickerCommand::OpenVertical,
    );
    bind(
        &mut keys,
        KeyCode::Char('t'),
        ctrl,
        PickerCommand::TogglePreview,
    );
    // Changed-file picker only. Other pickers pass this key to the prompt.
    bind(
        &mut keys,
        KeyCode::Char('g'),
        ctrl,
        PickerCommand::CycleChanges,
    );
    // Changed-file picker only, like `CycleChanges`.
    bind(
        &mut keys,
        KeyCode::Char('r'),
        alt,
        PickerCommand::ReviewJump,
    );
    bind(&mut keys, KeyCode::Esc, none, PickerCommand::Close);
    bind(&mut keys, KeyCode::Char('c'), ctrl, PickerCommand::Close);
    keys
}

pub fn overlay_picker_keys(
    keys: &mut HashMap<KeyEvent, PickerCommand>,
    overlay: HashMap<KeyEvent, PickerCommand>,
) {
    for (key, command) in overlay {
        if command == PickerCommand::Nop {
            keys.remove(&key);
        } else {
            keys.insert(key, command);
        }
    }
}
