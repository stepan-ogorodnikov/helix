# Fork

This is a soft fork of Helix. New fork behavior does not grow the files upstream rewrites most often. A field, a match arm, or a command-table row may stay in those files, because that is where upstream applies the change. A new block of logic goes in a file upstream has never had.

Hot files:

- `helix-term/src/commands.rs`
- `helix-term/src/commands/typed.rs`
- `helix-view/src/editor.rs`

## Hooks

Mark each hook with `// FORK:` so a search lists every seam.

A static command stays as a wrapper in `commands.rs`. `static_commands!` binds the function by name in that module. Move the body to `helix-term/src/commands/<name>.rs` and call it from the wrapper. `changed_file_picker` is the example.

A typable command keeps its row in `TYPABLE_COMMAND_LIST`. Point `fun` at the moved function. If the body calls a private helper in `typed.rs`, nest the module at `helix-term/src/commands/typed/<name>.rs`. A child module can call the parent's private items, so the helper stays private. `:yank-diagnostics` calls `yank_register` this way.

A config field stays on the upstream struct. Move the table to a sibling module and re-export it, so existing paths keep working. `Config::picker_keys` re-exports `picker_keys::PickerCommand` this way.

Move tests with the body. Remove imports the hot file no longer uses.
