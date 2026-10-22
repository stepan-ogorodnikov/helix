# Fork features

What this fork adds to Helix or changes in it, one entry per feature.
`AGENTS.md` has the rules for writing code here. `fork/<feature>.md` has a feature's plan.

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

## Features

Each entry says what the feature adds, and where its code, tests, and book pages are.
"Relies on" names upstream code the feature copies or depends on. A cherry-pick that changes it has to check the feature (`agents/UPSTREAM.md`).

### picker-keys

- Adds: `[keys.picker]` in `config.toml`.
  The built-in picker bindings stay, entries override them, and `nop` returns a key to the query prompt.
- Code: the table in `helix-view/src/editor/picker_keys.rs`, re-exported from `editor.rs` beside the `Config::picker_keys` field.
  `helix-term/src/config.rs` parses `[keys.picker]` and merges it over the defaults, and the key handler in `helix-term/src/ui/picker.rs` looks commands up in the table.
- Book: `book/src/keymap.md`.
- Tests: `picker_keys_overlay_defaults_and_nop_unbinds` in `config.rs`. No integration test.

### changed-files-views

- Adds: the changed-files picker (`Space g`) lists unstaged, staged, or all changes.
  It opens on unstaged. `Ctrl-g` (`cycle_changes`) cycles the list, and the current list shows beside the match count.
- Code: the picker in `helix-term/src/commands/changed_file.rs`.
  The status walk in `helix-vcs/src/git.rs` also reports staged changes (`status`, `staged_file_change`, `absolute_path`), and `for_each_changed_file` in `helix-vcs/src/lib.rs` passes each change's `ChangeKind`.
  `helix-vcs/src/status.rs` has `ChangeKind`, `ChangeView`, and `visible_changes`. `helix-term/src/ui/picker.rs` has the status label, the `on_cycle_changes` hook, and `replace_items`.
- Book: `book/src/keymap.md`.
- Tests: `changed_files_include_staged_and_unstaged` in `helix-vcs/src/git/test.rs`, `all_keeps_one_entry_and_prefers_unstaged` in `status.rs`.
- Relies on: upstream's status walk in `git.rs` and `lib.rs`. Upstream's #9951 and #16252, both open, rewrite it, so taking either means redoing the staged walk on top.

### yank-diagnostics

- Adds: `:yank-diagnostics` copies every diagnostic in the buffer as `path:line:col: severity: message`, to the clipboard by default.
  Upstream's `:yank-diagnostic` only copies the ones under the cursor.
- Code: `helix-term/src/commands/typed/yank_diagnostics.rs`, a child of `typed.rs`, so it can call the private `yank_register`.
- Book: `book/src/generated/typable-cmd.md`, generated.
- Tests: two unit tests in `yank_diagnostics.rs`.

### align-message

- Adds: the message below the statusline is drawn one column in, under the statusline's first text.
  Upstream draws it from column 0, while the statusline's elements pad themselves with a leading space.
- Code: the indent where `render` in `helix-term/src/ui/editor.rs` draws the status message.
- Tests: `status_message_starts_under_the_statusline_text` in `helix-term/tests/test/statusline.rs` reads the drawn bottom row.
  It reaches the screen through `Application::terminal_buffer`, a test-only accessor under the `integration` feature.
- Rulings: the indent is always on, with no config key, by the human's call.
- Gaps: the command line prompt (`:`) on the same row still starts at column 0.

### fork-infra

- Plan: `fork/fork-infra.md`, and `fork/hard-fork.md` for the move to a hard fork.
- Adds: the agent rules in `AGENTS.md` and `agents/`, this list, the `pre-commit` hook, and `tools/fork/land.sh`, the only CI.
- Files: `AGENTS.md`, `FORK.md`, `fork/*.md`, `agents/`, `tools/fork/`.
- Removes: upstream's `.github/workflows/`, `.github/actions/`, `.github/dependabot.yml`, and `.github/FUNDING.yml`.
  `docs/CONTRIBUTING.md` no longer lists `build.yml` among the places that name the MSRV.

### review-stream

- Plan: `fork/review-stream.md`, done through phase 9f.
  The completion spike's prototype is on the local branch `spike/review-completion`, which is never pushed.
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
- Code: `helix-view/src/review.rs` and `review/{layout,reconcile,forward,gutter,diagnostics,lend,log,sync,syntax,words,complete}.rs`, `helix-term/src/commands/review.rs` and `review/{lsp,stage,restore}.rs`, `helix-term/src/handlers/completion/request/review.rs`, `helix-term/src/handlers/signature_help/review.rs`, `helix-term/src/commands/typed/review_write.rs`, `helix-term/src/keymap/review.rs`, `helix-term/src/ui/text_decorations/review.rs`, and `helix-vcs/src/git/index.rs` with the `helix-vcs/src/index.rs` facade.
  It hooks in at:
  - `helix-view/src/editor.rs`: the `Editor::review` field, and a condition in `Editor::switch` that keeps the stream when you switch away from it. Without it, switching away deletes the stream, an unmodified buffer with no path.
  - `helix-view/src/gutter.rs`: the stream's branch at the top of `line_numbers`, `diff`, and `diagnostic`.
  - `helix-term/src/ui/editor.rs`: the `review_keys` field, `review::sync` and the keymap swap at the top of `handle_event`, `review::sync` at the top of `render`, and the review's decoration and overlays in `render_view`.
  - `helix-term/src/handlers/completion/request.rs`: the review's requests in `request_completions`, and its own re-request at the top of `request_incomplete_completion_list`.
  - `helix-term/src/handlers/completion.rs`: the file's trigger characters in `trigger_auto_completion`.
  - `helix-term/src/ui/completion.rs`: previews off for the review in `Completion::new`, and `review::accept_item` in the menu's accept arm.
  - `helix-term/src/handlers/signature_help.rs`: the lend at the top of `request_signature_help`, and the file's trigger characters in `signature_help_post_insert_char_hook`.
  - `helix-term/src/commands/typed.rs`: the seam at the top of `write_impl` that sends `:w` in the review to `typed/review_write.rs`.
  - `helix-term/src/ui/picker.rs`: the `on_review_jump` hook, its builder, and its key arm.
    `picker_keys.rs` (`picker-keys`) has `ReviewJump` and its `Alt-r` default, and `changed_file.rs` (`changed-files-views`) sets the hook.
- Book: `book/src/keymap.md` (Review), `book/src/themes.md` (the review's scopes), and `book/src/generated/static-cmd.md` and `typable-cmd.md`, generated.
- Tests: unit tests in `review.rs`, `review/layout.rs`, `review/reconcile.rs`, `review/forward.rs`, `review/syntax.rs`, `review/words.rs`, `review/complete.rs`, `helix-vcs/src/git/index.rs`, and `commands/review/stage.rs`. Integration tests in `helix-term/tests/test/review.rs` over a temporary git repository. `review/rust_analyzer.rs` runs completion against rust-analyzer. It's ignored, since it changes the working directory, and runs alone at a manual check: `cargo integration-test -- --ignored rust_analyzer --test-threads=1`.
- Relies on:
  - `goto_*`, `hover`, and `code_action` in `helix-term/src/commands/lsp.rs` run with the view lent to the row's file. A cherry-pick that makes their answers read the focused buffer breaks that.
  - The review's rename in `commands/review/lsp.rs` follows `rename_symbol`'s request and edit, without `prepareRename`. A cherry-pick that changes them updates it.
  - `request/review.rs` calls the private `request_completions_from_language_server`, and copies the completion context `request_completions` builds inline. A cherry-pick that changes either updates it.
  - `review::accept_item` sits in `ui/completion.rs`'s accept arm. A cherry-pick that changes how the arm turns an item into a transaction rereads it.
  - The signature help lend relies on `request_signature_help` reading the focused buffer only before its request goes out. A cherry-pick that changes that rereads it.
  - `typed/review_write.rs` copies `write_all_impl`'s save chain. A cherry-pick that changes the chain updates the copy.
  - `imara-diff` in `helix-view/Cargo.toml` moves together with helix-core's and helix-vcs's.
- Rulings:
  - The dropped diff review feature's plan and code stay under the local tag `archive/diff-review`. The tag is never pushed.
  - Staging runs the `git` binary: `git hash-object --path` and `git update-index`, which convert text the way `git add` does. Upstream reads diffs with gix only and never needs it.
  - The stream keeps itself read-only from `render`, not from event hooks, so one check catches edits from keys, typable commands, and paste before they're drawn.
  - The stream sets `gutters.line_numbers.min_width` on the views that show it, because `GutterType::width` gets no `&Editor`.
  - The review's LSP keys run upstream's own commands with the focused view pointed at the row's file for the length of the key, since their answers never read the buffer they asked from. They're built in the review's keymap and aren't in the command table.
  - `u` in the review walks the review's own log of the files' revisions, since the stream's history is cleared on every sync. When a file moved on since a step, the undo stops and names it, rather than undoing the file's own latest change.
