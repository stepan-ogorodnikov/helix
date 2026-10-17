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
- `origin/fork` only fast-forwards, except when the stack moves onto a new upstream.

## Setup

In a new clone:

```sh
git remote add upstream https://github.com/helix-editor/helix.git
gh repo set-default stepan-ogorodnikov/helix
git config core.hooksPath tools/fork/hooks
```

The hooks run on commit.
`pre-commit` formats the crates the staged Rust files belong to, and runs clippy on them.
`commit-msg` checks the `Patch:` trailer on `fork`.

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
- Files: `AGENTS.md`, `FORK.md`, `fork/*.md`, `tools/fork/hooks/`. All fork-owned. Upstream has never had these paths.
- Drop when: never.
