# Fork patches

What this fork changes against upstream Helix, one entry per feature.
`AGENTS.md` has the rules for writing fork code. `fork/<feature>.md` has a feature's plan.

Update a feature's entry in the same commit as the feature.

Fork point: upstream `ba40e547`, 2026-09-29.

## Branches

- `main` is the fork's line, the branch to build, and the default branch on GitHub.
- `origin/main` only fast-forwards, through `tools/fork/land.sh`. Nothing force-pushes it.
- `upstream/master` is upstream's line. `git merge-base main upstream/master` is the fork point, since cherry-picks don't change it.

## Setup

In a new clone:

```sh
git remote add upstream https://github.com/helix-editor/helix.git
git remote set-url --push upstream no_push
gh repo set-default stepan-ogorodnikov/helix
git config core.hooksPath tools/fork/hooks
```

`pre-commit` runs on commit. It formats the crates the staged Rust files belong to, and runs clippy on them.

## Build

`cargo install --path helix-term --locked` installs `hx` from the checkout.
`book/src/building-from-source.md` says how to point it at the `runtime` directory.

## Patches

### picker-keys

- Status: active.
- Adds: `[keys.picker]` in `config.toml`.
  The built-in picker bindings stay, entries override them, and `nop` returns a key to the query prompt.
- Upstream files:
  - `helix-view/src/editor.rs`: the `Config::picker_keys` field, its default, and `mod picker_keys;` with a re-export.
  - `helix-term/src/config.rs`: parsing `[keys.picker]` and merging it over the defaults.
  - `helix-term/src/ui/picker.rs`: the key handler looks commands up in the table.
  - `book/src/keymap.md`.
- Fork-owned: `helix-view/src/editor/picker_keys.rs`.
- Tests: `picker_keys_overlay_defaults_and_nop_unbinds` in `config.rs`. No integration test.
- Upstream refs: #5505 (key remapping for compositor components), open.
- Drop when: upstream makes picker keys configurable.
- Rulings: the key table lives outside `editor.rs`. Only the field and the re-export stay there.
- Gaps: the seams in `config.rs` and `ui/picker.rs` have no `FORK:` markers.

### changed-files-views

- Status: active.
- Adds: the changed-files picker (`Space g`) lists unstaged, staged, or all changes.
  It opens on unstaged. `Ctrl-g` (`cycle_changes`) cycles the list, and the current list shows beside the match count.
- Upstream files:
  - `helix-vcs/src/git.rs`: the status walk also reports staged changes (`status`, `staged_file_change`, `absolute_path`).
    About 130 changed lines, the fork's largest block of logic in an upstream file.
  - `helix-vcs/src/lib.rs`: `for_each_changed_file` passes each change's `ChangeKind`, plus re-exports.
  - `helix-vcs/src/status.rs`: `ChangeKind`, `ChangeView`, `visible_changes`.
  - `helix-term/src/ui/picker.rs`: the status label, the `on_cycle_changes` hook, `replace_items`.
  - `helix-term/src/commands.rs`: the `changed_file_picker` wrapper.
  - `book/src/keymap.md`.
- Fork-owned: `helix-term/src/commands/changed_file.rs`.
- Tests: `changed_files_include_staged_and_unstaged` in `helix-vcs/src/git/test.rs`, `all_keeps_one_entry_and_prefers_unstaged` in `status.rs`.
- Upstream refs: #9951 and #16252 rewrite the diff-base code in `git.rs` and `lib.rs`. Both open.
- Drop when: upstream's changed-files picker shows staged changes.
- Gaps: the seams in the `helix-vcs` files and `ui/picker.rs` have no `FORK:` markers.
  Expect conflicts in `git.rs` and `lib.rs` when #9951 or #16252 merges.

### yank-diagnostics

- Status: active.
- Adds: `:yank-diagnostics` copies every diagnostic in the buffer as `path:line:col: severity: message`, to the clipboard by default.
  Upstream's `:yank-diagnostic` only copies the ones under the cursor.
- Upstream files:
  - `helix-term/src/commands/typed.rs`: the command row and `mod yank_diagnostics;`.
  - `helix-term/Cargo.toml`: `slotmap`.
  - `book/src/generated/typable-cmd.md`, generated.
- Fork-owned: `helix-term/src/commands/typed/yank_diagnostics.rs`.
- Tests: two unit tests in `yank_diagnostics.rs`.
- Drop when: upstream adds a command that yanks all diagnostics.

### align-message

- Status: active.
- Adds: the message below the statusline is drawn one column in, under the statusline's first text.
  Upstream draws it from column 0, while the statusline's elements pad themselves with a leading space.
- Upstream files:
  - `helix-term/src/ui/editor.rs`: the indent where `render` draws the status message.
  - `helix-term/src/application.rs`: `terminal_buffer`, a test-only accessor to the drawn screen, under the `integration` feature.
  - `helix-term/tests/integration.rs`: `mod fork_statusline;`.
- Fork-owned: `helix-term/tests/test/fork_statusline.rs`.
- Tests: `status_message_starts_under_the_statusline_text` in `fork_statusline.rs` reads the drawn bottom row.
- Drop when: upstream lines the message up with the statusline.
- Rulings: the indent is always on, with no config key, by the human's call.
  Dropping the feature's commit brings back upstream's layout.
- Gaps: the command line prompt (`:`) on the same row still starts at column 0.

### fork-infra

- Status: active.
- Plan: `fork/fork-infra.md`, and `fork/hard-fork.md` for the move to a hard fork.
- Adds: the agent rules in `AGENTS.md` and `agents/`, this ledger, the `pre-commit` hook, and `tools/fork/land.sh`.
- Files: `AGENTS.md`, `FORK.md`, `fork/*.md`, `agents/`, `tools/fork/`. All fork-owned. Upstream has never had these paths.
- Removes: upstream's `.github/workflows/`, `.github/actions/`, `.github/dependabot.yml`, and `.github/FUNDING.yml`.
  `land.sh` is the only CI. `docs/CONTRIBUTING.md` no longer lists `build.yml` among the places that name the MSRV.
- Drop when: never.

### review-stream

- Status: active. The read-only review, phases 1–6 of `fork/review-stream.md`, is done. Editing in the review, phase 7, is done. Undo and saving in the review, phase 8, are done. Goto, references, hover, code actions, and rename from it, and its files' diagnostics, phases 9a–9c, are done. The completion spike, 9d, is done: its prototype is on the local branch `spike/review-completion`, which is never pushed. Completion and signature help from the files' language servers, 9e and 9f, are done.
- Plan: `fork/review-stream.md`.
- Adds: `:review` shows every changed file's hunks against `HEAD` in one buffer, or with `unstaged` or `staged` against the index, with old and new line numbers and `+`/`-` signs in the gutter.
  An edit to added or unchanged lines, new and deleted lines too, goes to the file's buffer as you type, loaded in the background if need be and closed with the review once saved, and the rows follow it. New lines and indents are the file's own. Any other edit is put back, and a staged review takes none.
  `u` and `U` in the review (`review_undo` and `review_redo`, swapped in for `undo`, `redo`, `earlier`, and `later`) undo and redo its edits and restores in every file they changed, and stop at a file that changed since in another way.
  `:w` in the review writes every file it shows with unsaved changes, through upstream's save chain, and `u` then undoes what the save changed first.
  `goto_definition`, `goto_declaration`, `goto_type_definition`, `goto_implementation`, `goto_reference`, `hover`, and `code_action` in the review ask the language server of the row's file, at the same place in it.
  `rename_symbol` in the review opens the review's own prompt, and the rename is a step of the review's undo log.
  Each file's diagnostics show on its rows, from its buffer, or from what its servers reported when it isn't open.
  Completion in the review offers the items of the row's file's language servers and opens on their trigger characters. Selecting an item there doesn't preview it in the text. An item's other edits, like an import, go to the file after the item, in the same undo step. Signature help in the review asks the row's file's language server, and opens on its trigger characters.
  `Enter` in it (`review_open`) opens the row's file at that line, and `Ctrl-o` comes back. On a row of unchanged lines, `Enter` shows them.
  An edit to a file's buffer shows in the review, with the cursor kept on its line. With a review open, `:review` switches to it, and `:review-refresh` lists and reads the changes again.
  In the review, `goto_next_change` and the other change motions select the review's hunks across files, on whatever keys they're bound to, and `]F`/`[F` go between files.
  `:stage-hunk` and `:unstage-hunk` write the hunks the selection touches to the index through the `git` binary, in a trusted workspace.
  `:review-restore` puts the base's text back over them in the files' buffers. `:review-context` sets the lines of context.
  In the changed-file picker, `review_jump` (`Alt-r`) jumps to the selected file in the review of the list's changes.
  Rows get syntax colors from parses of each file's whole base and source.
  Removed and added rows get tints, headers a rule, and changed words a mark, from theme scopes: `diff.{plus,minus}.view`, `diff.delta.view.text`, and the fork's `ui.review.header` and `ui.review.gap`.
- Upstream files:
  - `helix-view/src/editor.rs`: the `Editor::review` field, its default, and one condition in `Editor::switch` that keeps the stream when you switch away from it.
  - `helix-view/src/gutter.rs`: the stream's branch at the top of `line_numbers`, `diff`, and `diagnostic`.
  - `helix-view/src/lib.rs`: `pub mod review;`.
  - `helix-view/Cargo.toml`: `imara-diff`. Bump it together with helix-core's and helix-vcs's.
  - `helix-term/src/ui/editor.rs`: the `review_keys` field and its default, `review::sync` and the keymap swap at the top of `handle_event`, `review::sync` at the top of `render`, and the review's decoration and overlays in `render_view`.
  - `helix-term/src/ui/text_decorations.rs`: `pub mod review;`.
  - `helix-term/src/commands.rs`: `pub(crate) mod review;`, the `review_*` rows, and their wrappers.
  - `helix-term/src/commands/lsp.rs`: no seam, but the review runs `goto_*`, `hover`, and `code_action` with the view lent to the row's file. A rebase that makes their answers read the focused buffer breaks that.
    The review's rename in `commands/review/lsp.rs` follows `rename_symbol`'s request and edit, without `prepareRename`. A rebase that changes them updates it.
  - `helix-term/src/handlers/completion/request.rs`: `pub(super) mod review;`, the review's requests in `request_completions`, and the review's own re-request at the top of `request_incomplete_completion_list`.
    `request/review.rs` calls the private `request_completions_from_language_server`, and copies the completion context `request_completions` builds inline. A rebase that changes either updates it.
  - `helix-term/src/handlers/completion.rs`: the file's trigger characters in `trigger_auto_completion`.
  - `helix-term/src/ui/completion.rs`: previews off for the review in `Completion::new`, and `review::accept_item` in the menu's accept arm. A rebase that changes how the accept arm turns an item into a transaction rereads them.
  - `helix-term/src/handlers/signature_help.rs`: `mod review;`, the lend at the top of `request_signature_help`, and the file's trigger characters in `signature_help_post_insert_char_hook`.
    The lend relies on `request_signature_help` reading the focused buffer only before its request goes out. A rebase that changes that rereads it.
  - `helix-term/src/commands/typed.rs`: the `:review`, `:review-refresh`, `:review-restore`, `:review-context`, `:stage-hunk`, and `:unstage-hunk` rows, `mod review_write;`, and the seam at the top of `write_impl` that sends `:w` in the review to `typed/review_write.rs`.
    That module copies `write_all_impl`'s save chain. A rebase that changes the chain updates the copy.
  - `helix-term/src/keymap.rs`: `pub mod review;`.
  - `helix-term/tests/integration.rs`: `mod fork_review;`.
  - `helix-vcs/src/git.rs`: `pub(crate) mod index;`. `helix-vcs/src/lib.rs`: `pub mod index;`.
  - `helix-term/src/ui/picker.rs`: the `on_review_jump` hook, its builder, and its key arm, next to `changed-files-views`' `on_cycle_changes`.
  - Fork files of other features: `helix-view/src/editor/picker_keys.rs` (`picker-keys`) gets `ReviewJump` and its `Alt-r` default, and `helix-term/src/commands/changed_file.rs` (`changed-files-views`) sets the hook.
  - `book/src/keymap.md` (Review), `book/src/themes.md` (the review's scopes), `book/src/generated/static-cmd.md` and `typable-cmd.md`, generated.
- Fork-owned: `helix-view/src/review.rs` and `review/{layout,reconcile,forward,gutter,diagnostics,lend,log,sync,syntax,words,complete}.rs`, `helix-term/src/commands/review.rs` and `review/{lsp,stage,restore}.rs`, `helix-term/src/handlers/completion/request/review.rs`, `helix-term/src/handlers/signature_help/review.rs`, `helix-term/src/commands/typed/review_write.rs`, `helix-term/src/keymap/review.rs`, `helix-term/src/ui/text_decorations/review.rs`, `helix-vcs/src/git/index.rs` and the `helix-vcs/src/index.rs` facade, `helix-term/tests/test/fork_review.rs` and `fork_review/rust_analyzer.rs`.
- Tests: unit tests in `review.rs`, `review/layout.rs`, `review/reconcile.rs`, `review/forward.rs`, `review/syntax.rs`, `review/words.rs`, `review/complete.rs`, `helix-vcs/src/git/index.rs`, and `commands/review/stage.rs`. Integration tests in `fork_review.rs` over a temporary git repository. `fork_review/rust_analyzer.rs` runs completion against rust-analyzer. It's ignored, since it changes the working directory, and runs alone at a manual check: `cargo integration-test -- --ignored rust_analyzer --test-threads=1`.
- Upstream refs: #15611 (multibuffer prototype), #15631 (side-by-side diff viewer, open), whose `diff.*.view` scopes phase 4 uses.
  #9951 and #16252 rewrite the diff-base code in `git.rs` and `lib.rs`. Both open.
- Drop when: upstream ships a multi-file diff view in one buffer.
- Rulings:
  - The dropped diff review feature's plan and code stay under the local tag `archive/diff-review`. The tag is never pushed.
  - Staging runs the `git` binary: `git hash-object --path` and `git update-index`, which convert text the way `git add` does. Upstream reads diffs with gix only and never needs it.
  - The stream keeps itself read-only from `render`, not from event hooks, so one check catches edits from keys, typable commands, and paste before they're drawn.
  - The stream sets `gutters.line_numbers.min_width` on the views that show it, because `GutterType::width` gets no `&Editor`.
  - The condition in `Editor::switch` was approved as the one seam inside an upstream function in a hot file. Without it, switching away deletes the stream, an unmodified buffer with no path.
  - The review's LSP keys run upstream's own commands with the focused view pointed at the row's file for the length of the key, since their answers never read the buffer they asked from. They're built in the review's keymap and aren't in the command table.
  - `u` in the review walks the review's own log of the files' revisions, since the stream's history is cleared on every sync. When a file moved on since a step, the undo stops and names it, rather than undoing the file's own latest change.
