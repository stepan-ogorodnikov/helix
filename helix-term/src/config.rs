use crate::keymap;
use crate::keymap::{merge_keys, KeyTrie};
use helix_loader::merge_toml_values;
use helix_view::editor::{default_picker_keys, overlay_picker_keys, PickerCommand};
use helix_view::input::KeyEvent;
use helix_view::{document::Mode, theme};
use serde::de::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::Error as IOError;
use std::str::FromStr;
use toml::de::Error as TomlError;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: Option<theme::Config>,
    pub keys: HashMap<Mode, KeyTrie>,
    pub editor: helix_view::editor::Config,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawKeys {
    pub modes: HashMap<Mode, KeyTrie>,
    /// Bindings from `[keys.picker]` only. Defaults are applied later.
    pub picker: HashMap<KeyEvent, PickerCommand>,
}

impl<'de> Deserialize<'de> for RawKeys {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut value = toml::Value::deserialize(deserializer).map_err(D::Error::custom)?;
        let picker = match value
            .as_table_mut()
            .ok_or_else(|| D::Error::custom("keys must be a table"))?
            .remove("picker")
        {
            Some(picker) => parse_picker_keys(picker).map_err(D::Error::custom)?,
            None => HashMap::new(),
        };
        let modes = HashMap::<Mode, KeyTrie>::deserialize(value).map_err(D::Error::custom)?;
        Ok(Self { modes, picker })
    }
}

fn parse_picker_keys(value: toml::Value) -> Result<HashMap<KeyEvent, PickerCommand>, String> {
    let table = value
        .as_table()
        .ok_or_else(|| "picker keymap must be a table of key = \"command\"".to_string())?;
    let mut keys = HashMap::new();
    for (key, command) in table {
        let key = KeyEvent::from_str(key).map_err(|err| err.to_string())?;
        let command = command
            .as_str()
            .ok_or_else(|| format!("picker binding for '{key}' must be a command name"))?;
        let command = PickerCommand::from_name(command)
            .ok_or_else(|| format!("unknown picker command '{command}'"))?;
        keys.insert(key, command);
    }
    Ok(keys)
}

fn apply_picker_keys(
    editor: &mut helix_view::editor::Config,
    overlays: impl IntoIterator<Item = HashMap<KeyEvent, PickerCommand>>,
) {
    let mut picker_keys = default_picker_keys();
    for overlay in overlays {
        overlay_picker_keys(&mut picker_keys, overlay);
    }
    editor.picker_keys = picker_keys;
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRaw {
    pub theme: Option<theme::Config>,
    pub keys: Option<RawKeys>,
    pub editor: Option<toml::Value>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: None,
            keys: keymap::default(),
            editor: helix_view::editor::Config::default(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigLoadError {
    BadConfig(TomlError),
    Error(IOError),
}

impl Default for ConfigLoadError {
    fn default() -> Self {
        ConfigLoadError::Error(IOError::new(std::io::ErrorKind::NotFound, "place holder"))
    }
}

impl Display for ConfigLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadError::BadConfig(err) => err.fmt(f),
            ConfigLoadError::Error(err) => err.fmt(f),
        }
    }
}

impl Config {
    pub fn load(
        global: Result<&String, ConfigLoadError>,
        local: Result<String, ConfigLoadError>,
    ) -> Result<Config, ConfigLoadError> {
        let global_config: Result<ConfigRaw, ConfigLoadError> =
            global.and_then(|file| toml::from_str(file).map_err(ConfigLoadError::BadConfig));
        let local_config: Result<ConfigRaw, ConfigLoadError> =
            local.and_then(|file| toml::from_str(&file).map_err(ConfigLoadError::BadConfig));
        let res = match (global_config, local_config) {
            (Ok(global), Ok(local)) => {
                let mut keys = keymap::default();
                let mut picker_overlays = Vec::new();
                if let Some(global_keys) = global.keys {
                    merge_keys(&mut keys, global_keys.modes);
                    picker_overlays.push(global_keys.picker);
                }
                if let Some(local_keys) = local.keys {
                    merge_keys(&mut keys, local_keys.modes);
                    picker_overlays.push(local_keys.picker);
                }

                let mut editor = match (global.editor, local.editor) {
                    (None, None) => helix_view::editor::Config::default(),
                    (None, Some(val)) | (Some(val), None) => {
                        val.try_into().map_err(ConfigLoadError::BadConfig)?
                    }
                    (Some(global), Some(local)) => merge_toml_values(global, local, 3)
                        .try_into()
                        .map_err(ConfigLoadError::BadConfig)?,
                };
                apply_picker_keys(&mut editor, picker_overlays);

                Config {
                    theme: local.theme.or(global.theme),
                    keys,
                    editor,
                }
            }
            // if any configs are invalid return that first
            (_, Err(ConfigLoadError::BadConfig(err)))
            | (Err(ConfigLoadError::BadConfig(err)), _) => {
                return Err(ConfigLoadError::BadConfig(err))
            }
            (Ok(config), Err(_)) | (Err(_), Ok(config)) => {
                let mut keys = keymap::default();
                let picker_overlay = if let Some(keymap) = config.keys {
                    merge_keys(&mut keys, keymap.modes);
                    keymap.picker
                } else {
                    HashMap::new()
                };
                let mut editor = config.editor.map_or_else(
                    || Ok(helix_view::editor::Config::default()),
                    |val| val.try_into().map_err(ConfigLoadError::BadConfig),
                )?;
                apply_picker_keys(&mut editor, [picker_overlay]);
                Config {
                    theme: config.theme,
                    keys,
                    editor,
                }
            }

            // these are just two io errors return the one for the global config
            (Err(err), Err(_)) => return Err(err),
        };

        Ok(res)
    }

    pub fn load_default() -> Result<Config, ConfigLoadError> {
        let global_config =
            fs::read_to_string(helix_loader::config_file()).map_err(ConfigLoadError::Error)?;
        let local_config = fs::read_to_string(helix_loader::workspace_config_file())
            .map_err(ConfigLoadError::Error);

        let phony_config = ConfigLoadError::Error(IOError::other("hacky placeholder"));
        let global_parsed = Config::load(Ok(&global_config), Err(phony_config))?;

        // We need to build a transient `WorkspaceTrust` just to ask whether the workspace is
        // trusted enough to load its `.helix/config.toml`. The persisted-trust file on disk is the
        // source of truth either way; this transient instance has an empty cache and is dropped
        // after the check.
        let trust = helix_loader::workspace_trust::WorkspaceTrust::new(
            (&global_parsed.editor.workspace_trust).into(),
        );
        if trust
            .query_current(helix_loader::workspace_trust::TrustQuery::LocalConfig)
            .is_trusted()
        {
            let mut merged = Config::load(Ok(&global_config), local_config)?;
            // editor.workspace-trust is global/user-scope only. Without this override, a
            // workspace's `.helix/config.toml` could set `level = "insecure"`; once the user trusted
            // *that* workspace, refresh_config would re-load with the override merged in and from
            // then on every subsequent workspace in the session would be implicitly trusted. Pin
            // the gate's own configuration to the global file.
            merged.editor.workspace_trust = global_parsed.editor.workspace_trust;
            Ok(merged)
        } else {
            Ok(global_parsed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Config {
        fn load_test(config: &str) -> Config {
            Config::load(Ok(&config.to_owned()), Err(ConfigLoadError::default())).unwrap()
        }
    }

    #[test]
    fn parsing_keymaps_config_file() {
        use crate::keymap;
        use helix_core::hashmap;
        use helix_view::document::Mode;

        let sample_keymaps = r#"
            [keys.insert]
            y = "move_line_down"
            S-C-a = "delete_selection"

            [keys.normal]
            A-F12 = "move_next_word_end"
        "#;

        let mut keys = keymap::default();
        merge_keys(
            &mut keys,
            hashmap! {
                Mode::Insert => keymap!({ "Insert mode"
                    "y" => move_line_down,
                    "S-C-a" => delete_selection,
                }),
                Mode::Normal => keymap!({ "Normal mode"
                    "A-F12" => move_next_word_end,
                }),
            },
        );

        assert_eq!(
            Config::load_test(sample_keymaps),
            Config {
                keys,
                ..Default::default()
            }
        );
    }

    #[test]
    fn keys_resolve_to_correct_defaults() {
        // From serde default
        let default_keys = Config::load_test("").keys;
        assert_eq!(default_keys, keymap::default());

        // From the Default trait
        let default_keys = Config::default().keys;
        assert_eq!(default_keys, keymap::default());
    }

    #[test]
    fn picker_keys_overlay_defaults_and_nop_unbinds() {
        use helix_view::editor::PickerCommand;
        use helix_view::input::KeyEvent;
        use std::str::FromStr;

        let config = Config::load_test(
            r#"
            [keys.picker]
            C-k = "previous"
            C-p = "nop"
            "#,
        );
        let keys = &config.editor.picker_keys;
        let ctrl_k = KeyEvent::from_str("C-k").unwrap();
        let ctrl_p = KeyEvent::from_str("C-p").unwrap();
        let ctrl_n = KeyEvent::from_str("C-n").unwrap();
        assert_eq!(keys.get(&ctrl_k), Some(&PickerCommand::Previous));
        assert!(!keys.contains_key(&ctrl_p));
        assert_eq!(keys.get(&ctrl_n), Some(&PickerCommand::Next));
    }
}
