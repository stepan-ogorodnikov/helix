# Fork

This is a soft fork of Helix. New fork behavior does not grow the files upstream rewrites most often. A field, a match arm, or a command-table row may stay in those files, because that is where upstream applies the change. A new block of logic goes in a file upstream has never had.

Hot files:

- `helix-term/src/commands.rs`
- `helix-term/src/commands/typed.rs`
- `helix-view/src/editor.rs`

A fork feature is opt-in or additive. Upstream defaults and config keep their meaning, so any feature can be dropped. Inside fork code, update the call sites and delete the old code instead of keeping a shim.

## Git

`master` mirrors `upstream/master`. Never commit to it. `fork` is `master` plus the fork's commits. Commit there.

Never push to `helix-editor/helix`, and never open or comment on its pull requests and issues.

In a fork clone, `gh` picks the parent repository until `gh repo set-default stepan-ogorodnikov/helix` has run. Check `gh repo set-default --view` before using `gh`.

## Commits

Write the subject in the imperative and in sentence case, end it with a period, and leave out upstream's `area:` prefix. Say why in the body.

End every commit on `fork` with one `Patch: <slug>` trailer, where the slug names a `FORK.md` entry. A commit serves one feature, so the feature can be dropped by its commits. `git log master..fork --grep '^Patch: <slug>$'` lists them.

The hooks in `tools/fork/hooks` format, run clippy, and check the trailer. Fix what they report instead of committing with `--no-verify`.

## Hooks

Mark each hook with `// FORK:` so a search lists every seam.

A static command stays as a wrapper in `commands.rs`. `static_commands!` binds the function by name in that module. Move the body to `helix-term/src/commands/<name>.rs` and call it from the wrapper. `changed_file_picker` is the example.

A typable command keeps its row in `TYPABLE_COMMAND_LIST`. Point `fun` at the moved function. If the body calls a private helper in `typed.rs`, nest the module at `helix-term/src/commands/typed/<name>.rs`. A child module can call the parent's private items, so the helper stays private. `:yank-diagnostics` calls `yank_register` this way.

A config field stays on the upstream struct. Move the table to a sibling module and re-export it, so existing paths keep working. `Config::picker_keys` re-exports `picker_keys::PickerCommand` this way.

Move tests with the body. Remove imports the hot file no longer uses.

## Ledger

`FORK.md` lists every fork feature: what it touches upstream, and when to drop it. Update a feature's entry in the same commit as the feature.

## Plans

A fork feature's plan lives in `fork/<feature>.md`. Commit the plan before the code. Each phase's commit marks the phase done in the plan and records what the work found.
