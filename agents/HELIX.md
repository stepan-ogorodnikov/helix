# Helix conventions for fork code

Upstream's docs come first. This guide points to them and adds what fork code needs on top.
`AGENTS.md` has the hot-file rule and the hook shapes.

## Upstream docs

- `docs/CONTRIBUTING.md`: building, `cargo integration-test`, `cargo xtask docgen`, `HELIX_LOG_LEVEL`, the MSRV policy.
- `docs/architecture.md`: what each crate owns.
- `book/src/`: the user docs.
  A fork feature updates the page that owns the behavior, in place, and its `FORK.md` entry lists the page.
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
| Commands | Static: `helix-term/src/commands.rs`. Typable: `helix-term/src/commands/typed.rs`. Fork bodies live in `commands/<name>.rs` and `commands/typed/<name>.rs`. |
| File explorer | `file_explorer` in `helix-term/src/ui/mod.rs` |
| Changed files, diffs | `helix-vcs/`. The fork's changed-file picker is `helix-term/src/commands/changed_file.rs`. |
| Drawing a view | `render_view` in `helix-term/src/ui/editor.rs`, and `helix-view/src/view.rs` |
| Virtual text | `helix-core/src/text_annotations.rs`, `helix-view/src/annotations/`, `helix-term/src/ui/text_decorations/` |

## Comments

Comment only the traps: code that another developer or agent would "fix" and break.
Don't say what the code does.
Start each comment in fork code with a tag:

- `// WHY:` code that looks wrong, redundant, or backwards, and isn't.
- `// INVARIANT:` a rule the code has to keep, such as an order, a lock scope, or a shape other code relies on.
- `// WORKAROUND:` a quirk of a dependency or of upstream code, and what would let the workaround go.
- `// FORK: <slug>`: a seam in an upstream file, as `AGENTS.md` (Hooks) says.

Doc comments (`///`, `//!`) need no tag.
Use them to orient a reader at a module or type: what it owns, and what it leaves to others.
Don't write them for ordinary functions.

Never add or change comments on upstream lines the change doesn't otherwise touch.

## Change discipline

- Make the smallest diff that does the job.
- Don't reformat, rename, reorder, or tidy upstream code. Each changed upstream line is a future conflict.
- Leave upstream bugs the change didn't cause. Mention them instead.
- Don't add parameters, options, or branches for cases nobody asked for.
