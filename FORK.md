# Fork patches

What this fork changes against upstream Helix, one entry per feature.
`AGENTS.md` has the rules for writing fork code. `fork/<feature>.md` has a feature's plan.

Update a feature's entry in the same commit as the feature.
An entry's heading is the `Patch:` trailer on its commits, so `git log master..fork --grep '^Patch: <slug>$'` lists them.
At a rebase, check each entry's "Drop when" against upstream, and drop the feature's commits when it holds.

Upstream base: `master` at `ba40e547`.

## Branches

- `master` mirrors `upstream/master`. Nothing else commits to it.
- `fork` is `master` plus the patches below. It's the branch to build.
- `origin/fork` only fast-forwards, through `tools/fork/land.sh`, except when the stack moves onto a new upstream.

## Setup

In a new clone:

```sh
git remote add upstream https://github.com/helix-editor/helix.git
gh repo set-default stepan-ogorodnikov/helix
git config core.hooksPath tools/fork/hooks
git config rerere.enabled true
```

The hooks run on commit.
`pre-commit` formats the crates the staged Rust files belong to, and runs clippy on them.
`commit-msg` checks the `Patch:` trailer on `fork`.
`rerere` remembers how a conflict was resolved, for the next rebase onto upstream (`agents/REBASE.md`).

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

### fork-infra

- Status: active.
- Plan: `fork/fork-infra.md`.
- Adds: the agent rules in `AGENTS.md` and `agents/`, this ledger, the commit hooks, and `tools/fork/land.sh`.
- Files: `AGENTS.md`, `FORK.md`, `fork/*.md`, `agents/`, `tools/fork/`. All fork-owned. Upstream has never had these paths.
- Drop when: never.

### review-stream

- Status: active, phases 1–5 of `fork/review-stream.md`.
- Plan: `fork/review-stream.md`.
- Adds: `:review` shows every changed file's hunks against `HEAD` in one read-only buffer, with old and new line numbers and `+`/`-` signs in the gutter.
  An edit in it is put back. `Enter` in it (`review_open`) opens the row's file at that line, and `Ctrl-o` comes back. On a row of unchanged lines, `Enter` shows them.
  An edit to a file's buffer shows in the review, with the cursor kept on its line. With a review open, `:review` switches to it, and `:review-refresh` lists and reads the changes again.
  In the review, `goto_next_change` and the other change motions select the review's hunks across files, on whatever keys they're bound to, and `]F`/`[F` go between files.
  Rows get syntax colors from parses of each file's whole base and source.
  Removed and added rows get tints, headers a rule, and changed words a mark, from theme scopes: `diff.{plus,minus}.view`, `diff.delta.view.text`, and the fork's `ui.review.header` and `ui.review.gap`.
- Upstream files:
  - `helix-view/src/editor.rs`: the `Editor::review` field, its default, and one condition in `Editor::switch` that keeps the stream when you switch away from it.
  - `helix-view/src/gutter.rs`: the stream's branch at the top of `line_numbers` and `diff`.
  - `helix-view/src/lib.rs`: `pub mod review;`.
  - `helix-view/Cargo.toml`: `imara-diff`. Bump it together with helix-core's and helix-vcs's.
  - `helix-term/src/ui/editor.rs`: the `review_keys` field and its default, the keymap swap at the top of `handle_event`, `review::sync` at the top of `render`, and the review's decoration and overlays in `render_view`.
  - `helix-term/src/ui/text_decorations.rs`: `pub mod review;`.
  - `helix-term/src/commands.rs`: `mod review;`, the `review_*` rows, and their wrappers.
  - `helix-term/src/commands/typed.rs`: the `:review` and `:review-refresh` rows.
  - `helix-term/src/keymap.rs`: `pub mod review;`.
  - `helix-term/tests/integration.rs`: `mod fork_review;`.
  - `book/src/keymap.md` (Review), `book/src/themes.md` (the review's scopes), `book/src/generated/static-cmd.md` and `typable-cmd.md`, generated.
- Fork-owned: `helix-view/src/review.rs` and `review/{layout,reconcile,gutter,sync,syntax,words}.rs`, `helix-term/src/commands/review.rs`, `helix-term/src/keymap/review.rs`, `helix-term/src/ui/text_decorations/review.rs`, `helix-term/tests/test/fork_review.rs`.
- Tests: unit tests in `review.rs`, `review/layout.rs`, `review/reconcile.rs`, `review/syntax.rs`, and `review/words.rs`. Integration tests in `fork_review.rs` over a temporary git repository.
- Upstream refs: #15611 (multibuffer prototype), #15631 (side-by-side diff viewer, open), whose `diff.*.view` scopes phase 4 uses.
- Drop when: upstream ships a multi-file diff view in one buffer.
- Rulings:
  - The dropped diff review feature's plan and code stay under the local tag `archive/diff-review`. The tag is never pushed.
  - The stream keeps itself read-only from `render`, not from event hooks, so one check catches edits from keys, typable commands, and paste before they're drawn.
  - The stream sets `gutters.line_numbers.min_width` on the views that show it, because `GutterType::width` gets no `&Editor`.
  - The condition in `Editor::switch` was approved as the one seam inside an upstream function in a hot file. Without it, switching away deletes the stream, an unmodified buffer with no path.
