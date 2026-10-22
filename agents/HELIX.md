# Helix conventions

Upstream's docs still describe the code. This guide points to them and adds the fork's own rules on top.

## Upstream docs

- `docs/CONTRIBUTING.md`: building, `cargo integration-test`, `cargo xtask docgen`, `HELIX_LOG_LEVEL`, the MSRV policy.
- `docs/architecture.md`: what each crate owns.
- `book/src/`: the user docs.
  A feature updates the page that owns the behavior, in place, and its `FORK.md` entry lists the page.
- `tools/fork/land.sh`: the fork's only CI, the checks upstream's `build.yml` ran at the fork point.
  A change under `runtime/` also needs the `cargo xtask` check for what it touches: `query-check`, `indent-check`, `highlight-check`, or `theme-check`.

Rust 1.90 is both the MSRV and the pinned toolchain (`rust-toolchain.toml`).
Don't use a std API that a later release stabilized.

`book/src/generated/` comes from `cargo xtask docgen`.
Run it after adding or renaming a command, and commit the result with the command.

## Where to start

| Area | Start in |
| --- | --- |
| Pickers | `helix-term/src/ui/picker.rs`. The fork's picker key table is `helix-view/src/editor/picker_keys.rs`. |
| Keymaps | `helix-term/src/keymap.rs`, `helix-term/src/keymap/default.rs` |
| Config | `helix-term/src/config.rs`, and `Config` in `helix-view/src/editor.rs` |
| Commands | Static: `helix-term/src/commands.rs`. Typable: `helix-term/src/commands/typed.rs`. Some bodies live in `commands/<name>.rs` and `commands/typed/<name>.rs`. |
| File explorer | `file_explorer` in `helix-term/src/ui/mod.rs` |
| Changed files, diffs | `helix-vcs/`. The fork's changed-file picker is `helix-term/src/commands/changed_file.rs`. |
| Drawing a view | `render_view` in `helix-term/src/ui/editor.rs`, and `helix-view/src/view.rs` |
| Virtual text | `helix-core/src/text_annotations.rs`, `helix-view/src/annotations/`, `helix-term/src/ui/text_decorations/` |

## Comments

Comment only the traps: code that another developer or agent would "fix" and break.
Don't say what the code does.
Start each comment you write with a tag:

- `// WHY:` code that looks wrong, redundant, or backwards, and isn't.
- `// INVARIANT:` a rule the code has to keep, such as an order, a lock scope, or a shape other code relies on.
- `// WORKAROUND:` a quirk of a dependency, and what would let the workaround go.

Doc comments (`///`, `//!`) need no tag.
Use them to orient a reader at a module or type: what it owns, and what it leaves to others.
Don't write them for ordinary functions.

## Change discipline

- Make the smallest diff that does the job.
- Change any code the job needs: fix it, rename it, or restructure it, upstream's included.
  Leave the rest alone, comments too, so the diff stays about one thing.
- Fix a bug the change didn't cause in a commit of its own, or mention it.
- Don't add parameters, options, or branches for cases nobody asked for.
