# Fork patches

What this fork changes against upstream Helix, one entry per feature.
`AGENTS.md` has the rules for writing fork code. `fork/<feature>.md` has a feature's plan.

Update a feature's entry in the same commit as the feature.
At a rebase, check each entry's "Drop when" against upstream, and drop the feature's commits when it holds.

Upstream base: `master` at `ba40e547`.

## picker-keys

- Status: active.
- Commits: `a437d59f`, `eb31e577`.
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

## changed-files-views

- Status: active.
- Commits: `1d1a31e0`, `3b978617`.
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

## yank-diagnostics

- Status: active.
- Commits: `e117b0d4`, `3b978617`.
- Adds: `:yank-diagnostics` copies every diagnostic in the buffer as `path:line:col: severity: message`, to the clipboard by default.
  Upstream's `:yank-diagnostic` only copies the ones under the cursor.
- Upstream files:
  - `helix-term/src/commands/typed.rs`: the command row and `mod yank_diagnostics;`.
  - `helix-term/Cargo.toml`: `slotmap`.
  - `book/src/generated/typable-cmd.md`, generated.
- Fork-owned: `helix-term/src/commands/typed/yank_diagnostics.rs`.
- Tests: two unit tests in `yank_diagnostics.rs`.
- Drop when: upstream adds a command that yanks all diagnostics.

## diff-review

- Status: active. Phases 1–3 done, phase 4 next. Plan: `fork/diff-review.md`.
- Commits: `cb307282`, `0ce7825e`, `6dd4eb67`, `fc6be72c`.
- Adds: `toggle_inline_diff`, `:diff-yank-base`, `:review`, `review_next_file`, `review_prev_file`, `review_next_hunk`, `review_prev_hunk`, and `start_review` (`Alt-r`) in the changed-files picker.
  Phase 4 adds `:stage-hunk` and `:unstage-hunk`.
- Upstream files: one-line seams, marked `// FORK:`. The plan's §7 lists them.
  `helix-view/src/view.rs`, `helix-view/src/lib.rs`, `helix-view/src/annotations.rs`, `helix-term/src/ui/editor.rs`, `helix-term/src/ui/text_decorations.rs`, `helix-term/src/ui/picker.rs`, `helix-term/src/commands.rs`, `helix-term/src/commands/typed.rs`, `helix-term/Cargo.toml` (`imara-diff`), `helix-term/tests/integration.rs`, `book/src/themes.md`, `book/src/keymap.md`.
  Phase 4 adds `helix-vcs/src/git.rs` and `helix-vcs/src/lib.rs`.
- Fork-owned: `helix-view/src/annotations/diff.rs`, `helix-view/src/review.rs`, `helix-term/src/ui/text_decorations/diff.rs`, `helix-term/src/ui/text_decorations/diff/words.rs`, `helix-term/src/commands/review.rs`, `helix-term/src/commands/typed/diff_yank_base.rs`, `helix-term/tests/test/fork_diff.rs`, `helix-term/tests/test/fork_diff/review.rs`.
- Tests: unit tests in `annotations/diff.rs` and `diff/words.rs`. Integration tests in `fork_diff.rs` and `fork_diff/review.rs`.
- Requires: from phase 4, staging runs the `git` binary and only works in a trusted workspace.
  Upstream reads diffs with gix and never runs `git`.
- Upstream refs: #15631 (side-by-side, open), #405, #9127 (inline diff, closed), #9951 and #16252 (diff base, open), #14434 (hunk picker, open).
- Drop when: never as a whole. Upstream closed inline diff (#9127) as a variant of #405.
  If #15631 merges, rebase onto it and reconcile its theme scopes and `]g` changes.
  If #9951 or #16252 merges, recheck the phase 4 seams in `helix-vcs`.
- Rulings:
  - Removed rows go above the new lines.
  - A review diffs against `HEAD` by default. `:review unstaged` uses the index.
  - Staging writes the index with `git hash-object --path` and `git update-index`, not `git apply --cached`.

## fork-infra

- Status: active.
- Commits: `716c86d5`, plus the commits that add plans and this file.
- Files: `AGENTS.md`, `FORK.md`, `fork/*.md`. All fork-owned. Upstream has never had these paths.
- Drop when: never.
