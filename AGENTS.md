# Fork

This is a soft fork of Helix. New fork behavior does not grow the files upstream rewrites most often. A field, a match arm, or a command-table row may stay in those files, because that is where upstream applies the change. A new block of logic goes in a file upstream has never had.

Hot files:

- `helix-term/src/commands.rs`
- `helix-term/src/commands/typed.rs`
- `helix-view/src/editor.rs`
- `helix-view/src/document.rs`

A fork feature is opt-in or additive. Upstream defaults and config keep their meaning, so any feature can be dropped. Inside fork code, update the call sites and delete the old code instead of keeping a shim.

## Git

`main` is the fork's line. Commit there. `upstream/master` is upstream's.

Never push to `helix-editor/helix`, and never open or comment on its pull requests and issues.

Push `main` only through `tools/fork/land.sh`, and only when the human asks. It runs the checks and fast-forwards `origin/main`. `tools/fork/land.sh --check` runs the same checks and pushes nothing. Nothing force-pushes `origin/main`.

Take a change from upstream only when the human asks, by `git cherry-pick -x`.

In a fork clone, `gh` picks the parent repository until `gh repo set-default stepan-ogorodnikov/helix` has run. Check `gh repo set-default --view` before using `gh`.

## Commits

Write the subject in the imperative and in sentence case, end it with a period, and leave out upstream's `area:` prefix. Say why in the body.

A commit does one thing, so it can be read and reverted alone.

The `pre-commit` hook in `tools/fork/hooks` formats and runs clippy. Fix what it reports instead of committing with `--no-verify`.

## Hooks

Mark each hook with `// FORK: <slug>`, where the slug names its `FORK.md` entry. `rg 'FORK:'` lists every seam, and `rg 'FORK: <slug>'` one feature's.

A static command stays as a wrapper in `commands.rs`. `static_commands!` binds the function by name in that module. Move the body to `helix-term/src/commands/<name>.rs` and call it from the wrapper. `changed_file_picker` is the example.

A typable command keeps its row in `TYPABLE_COMMAND_LIST`. Point `fun` at the moved function. If the body calls a private helper in `typed.rs`, nest the module at `helix-term/src/commands/typed/<name>.rs`. A child module can call the parent's private items, so the helper stays private. `:yank-diagnostics` calls `yank_register` this way.

A config field stays on the upstream struct. Move the table to a sibling module and re-export it, so existing paths keep working. `Config::picker_keys` re-exports `picker_keys::PickerCommand` this way.

Move tests with the body. Remove imports the hot file no longer uses.

## Ledger

`FORK.md` lists every fork feature: what it touches upstream, and when to drop it. Update a feature's entry in the same commit as the feature.

## Plans

A fork feature that takes more than one commit gets a plan in `fork/<feature>.md`. A small fix doesn't.

Ask the human every question whose answer changes the plan, like scope, order, or approach, instead of guessing. Commit the plan before the code. Implement only after the human approves the plan and asks for the work.

Each phase is one commit that builds and passes its tests. Order the phases so each starts from what the previous one left, and put the riskiest as early as its dependencies allow. If a phase's subject can't say what it does in one line, split it. Book pages ride with the phase that makes them true.

Each phase's commit marks the phase done in the plan and records what the work found.

## Guides

- `agents/HELIX.md`: upstream's docs and checks, where code lives, comment tags. Read it before changing Rust code.
- `agents/TESTING.md`: which tests to write, where they go, and how to run them. Read it before writing or changing a test.
- `agents/REVIEW.md`: how to review a diff on `main`. Read it for any review.
- `agents/REBASE.md`: moving the stack onto a new upstream.
- `agents/VERIFY.md`: how to hand a user-visible change back for a check. Never run `hx` interactively or drive it through a terminal.
