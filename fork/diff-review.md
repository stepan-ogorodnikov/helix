# Helix fork: a native diff review mode

Snapshot: fork branch `fork` at `eb31e577`, on upstream `master` at `ba40e547`.
Reference tool: [Hunk](https://www.hunk.dev/) (`modem-dev/hunk`).
Upstream PR state as of 2026-10-14.

## Bottom line

It's feasible. Most of the machinery is already in the tree.

- **Build:** an inline (unified) diff overlay on the real buffer, plus a review session that walks changed files.
  You navigate with normal Helix keys and edit the real file, with LSP still running.
  Hunk's docs only mention opening `$EDITOR` from its file browser.
- **Don't build side-by-side.** Upstream PR #15631 is a vimdiff-style split viewer.
  It's open, active, and archseer is test-driving it. Adopt it if it merges.
- **Don't build a continuous multi-file buffer** (Zed-style multibuffer).
  It changes how a view relates to documents, and the existing prototype puts most of its code in hot files.
- **Staging hunks** fits as a later phase.
  It needs the index as a diff base, and upstream is rewriting exactly that plumbing right now.
- **Size:** roughly 1.1–1.7k fork-owned lines for phases 1–3. About a dozen one-line seams.
  Most seams are in `view.rs` and `ui/editor.rs`, not in the hot files.

## 1. What Hunk is

- A TUI diff viewer for reviewing agent-written changes, built on OpenTUI.
- Multi-file review stream with a sidebar. Split, stack (unified), and auto layouts.
- `[`/`]` move between hunks, and `1`/`2`/`0` switch the layout.
- Syntax highlighting and themes. No word-level or structural diff is listed.
- Agent notes render inline above hunks. Agents drive a live session through `hunk session` and a local daemon.
- Git, jj, and Sapling. Pager and difftool modes, and `--watch` reload.
- No staging. No editing, except `e` in `hunk open`, which launches `$EDITOR`.

The parts worth having in Helix: walking hunks across many files, seeing removed lines next to the new ones, and notes.
The parts Helix does better once this exists: editing in place, LSP, and Helix motions and search over the code under review.

## 2. What Helix already has

| Capability | Where | Notes |
| --- | --- | --- |
| Per-document diff against `HEAD` | `helix-vcs/src/diff.rs` (`DiffHandle`, `Diff`) | Holds the base `Rope`, a doc snapshot, and `imara_diff` hunks. Updated async: 12 ms sync timeout, 96 ms debounce. |
| Diff base loading | `helix-view/src/document.rs:1313`, `helix-view/src/editor.rs:2135` | Only on open and `:reload`. After a commit, gutters stay stale until a reload. |
| Base = `HEAD` tree blob | `helix-vcs/src/git.rs` `get_diff_base` | Not the index. Gix only, no `git` binary. |
| Gutter markers | `helix-view/src/gutter.rs` `diff` | Added, modified, and removed markers. |
| Hunk motions | `]g` `[g` `]G` `[G`, textobject `mig`/`mag` | `goto_next_change_impl` in `commands.rs`. |
| Revert hunk | `:reset-diff-change` (`:diffget`) | `typed.rs:2691`. Pulls base text for hunks under the selection. |
| Changed-file list | fork's `changed_file_picker` (`commands/changed_file.rs`) | Unstaged, staged, or all, from a gix status walk. |
| Virtual lines | `LineAnnotation` (`helix-core/src/text_annotations.rs:115`) | Its doc comment names side-by-side diff alignment as a use case. |
| Precedent for virtual text | Inline diagnostics: `helix-view/src/annotations/diagnostics.rs` and `helix-term/src/ui/text_decorations/diagnostics.rs` | A `LineAnnotation` reserves rows in `View::text_annotations`, and a `Decoration` paints them in `render_view`. `softwrapped_dimensions` computes wrapped heights. |
| Decorations can paint the gutter | `render_gutter` is itself a `Decoration` | So removed rows can show `-` and old line numbers. |
| Diff grammar, stdin buffers | `languages.toml` `diff`, `new_file_from_stdin` | `git diff \| hx` then `:lang diff` already works as a read-only stream. |

Missing: rendering removed text, the index as a base, staging, cross-file hunk navigation, refreshing the base after a commit, and a file watcher (none in this tree).

## 3. Upstream landscape

| Ref | State | What it means for the fork |
| --- | --- | --- |
| [#15631](https://github.com/helix-editor/helix/pull/15631) side-by-side diff viewer | Open. 32 commits, +4333/−185, last push 2026-09-29. archseer: "Great work… I'll test drive the branch", and proposed theme defaults. No formal review yet. | It's the side-by-side mode, so let upstream own it. It bundles #9951. It puts ~1170 lines into upstream files (`ui/editor.rs` +622, `typed.rs` +283, `view/editor.rs` +160, `commands.rs` +109). `git merge-tree fork <PR>` conflicts in `commands.rs`, `helix-vcs/src/git.rs`, and `helix-vcs/src/lib.rs`. Don't cherry-pick it. Reuse its theme scopes: `diff.{plus,minus,delta}.view`, `diff.delta.view.text`. |
| [#9951](https://github.com/helix-editor/helix/pull/9951) diff source autodetection/caching, [#16252](https://github.com/helix-editor/helix/pull/16252) `editor.diff-base`, [#15938](https://github.com/helix-editor/helix/pull/15938) gitbase picker, [#12022](https://github.com/helix-editor/helix/pull/12022) jj provider | All open | The diff-base plumbing in `helix-vcs/src/{lib,git}.rs` is under active rewrite. Limit fork VCS code there to one-line seams (5.3). |
| [#9127](https://github.com/helix-editor/helix/issues/9127) inline git diff | Closed as a variant of #405 | pascalkuthe: "We are also unlikely to implement 5 different diff modes." If side-by-side wins upstream, inline stays fork-only. Record it in the patch ledger (see 5.3). |
| [#405](https://github.com/helix-editor/helix/issues/405) side-by-side diffing | Open since 2021 | Tracking issue for #15631. |
| [#5166](https://github.com/helix-editor/helix/issues/5166), [#6101](https://github.com/helix-editor/helix/issues/6101) git actions | Open | pascalkuthe's plan: staging as `:diffput` onto a side-by-side index buffer. Nothing landed. |
| [#14434](https://github.com/helix-editor/helix/pull/14434) hunk picker, [#13383](https://github.com/helix-editor/helix/issues/13383) | Open | A per-file hunk picker. Small, and complements this work. |
| [#15611](https://github.com/helix-editor/helix/issues/15611) multibuffer + matoous's `md/multibuffer` branch | Prototype, last commit 2026-07-23 | ~1750 lines, mostly `view/editor.rs` +588 and `document.rs` +259. Proves a continuous stream is possible, but not at fork scale. |

## 4. Design options

| Option | You get | Cost | Hot-file exposure | Verdict |
| --- | --- | --- | --- | --- |
| **A. Inline overlay on the real buffer**: removed lines as virtual rows, changed lines tinted | Helix keys, real edits, LSP, `]g`, `:diffget`, search | M | Low: seams in `view.rs` (3 commits/yr) and `ui/editor.rs` (9/yr) | **Build** |
| **B. Side-by-side** (vimdiff) | Both sides as buffers, so the old side is selectable too, plus `:diffput` | L | High | **Wait for #15631** |
| **C. Synthetic review buffer**: unified diff text across all files in a scratch buffer | One stream, Helix motions and search over all hunks | S | None | **Maybe later.** Read-only. Edits don't flow back. `git diff \| hx` already covers most of it. |
| **D. Multibuffer**: excerpts of many documents in one view, edits go back | Hunk's stream with real editing | XL | Very high: views are one-document today | **Skip** |

A plus a review session covers what Hunk does, minus the split layout and the sidebar.
B arrives for free if upstream merges it, and A still complements it.

## 5. Recommended design

### 5.1 Inline diff (phases 1–2)

What a view looks like with the toggle on:

```
 12 ▍ fn render(&self, area: Rect) {
    -     let width = area.width - 2;                 ← virtual row: base text, diff.minus.view
 13 ▍     let width = area.width.saturating_sub(2);   ← real line: editable, diff.delta.view
 14       let height = area.height;
```

How it works:

- Removed rows go above the new lines, git and Hunk style.
- A `LineAnnotation` (new `helix-view/src/annotations/diff.rs`) reserves `hunk.before.len()` rows after line `hunk.after.start - 1`.
  Under soft-wrap it reserves wrapped heights via `softwrapped_dimensions`.
- It must be added in `View::text_annotations`, not only at render time.
  Scrolling and visual `j`/`k` call `text_annotations` too, so the row math has to match.
  Inline diagnostics work the same way.
- A `Decoration` (new `helix-term/src/ui/text_decorations/diff.rs`) does the painting:
  - Removed rows from the base `Rope`, with old line numbers in the gutter columns.
  - `decorate_line` tints added and modified lines.
- Phase 2 added:
  - Soft wrap. A removed line wraps like a document line, with the wrap indicator.
    The annotation sizes rows in `insert_virtual_lines`, so only lines a traversal reaches get formatted.
    The decoration reserves the same `row_height` and draws through a `DocumentFormatter`.
  - Word-level highlights: an `imara_diff` Myers diff over word tokens, per modified hunk in view (`text_decorations/diff/words.rs`).
    The document side is an overlay layer, pushed before the selection overlays. The decoration paints the removed side with its rows.
  - Syntax highlighting of removed rows. The base is parsed on the blocking pool and cached on the view (`InlineDiffState`) until the base or the language changes.
    The finished parse requests a redraw. Until then, rows have no syntax colors.
- Don't keep a `Diff` inside the annotation. A `Diff` holds a `parking_lot` read lock on the diff.
  - `render_view` keeps the annotations alive from `ui/editor.rs:94` to `:213`, and the gutter takes a second read lock at `:172` (`gutter.rs:103`).
  - If the diff worker asks for its write lock in between (`diff/worker.rs:73`), the fair lock makes the gutter's read wait behind the writer.
    The writer waits for the annotation's read lock, so the editor hangs.
  - This can happen: base updates never wait for the frame, and doc updates stop waiting after 12 ms.
  - So the constructor calls `load()` once, copies the hunks, clones the base `Rope` (cheap), and drops the lock.
  - Clamp indices. The copy can lag the buffer by a frame while you type, the same as the gutter.
- Theme scopes, from #15631: `diff.{plus,minus,delta}.view`, `diff.delta.view.text`.
  - Removed rows use `theme.get("diff.minus.view")`.
    It falls back to `diff.minus`, a text color in 140 of 218 bundled themes, which suits removed text.
  - Syntax colors go under the removed style, so they only show when `diff.minus.view` sets no foreground (a background tint, as #15631 proposes).
    With the fallback text color, removed rows stay one color, and the base isn't parsed.
  - Changed words use `diff.delta.view.text`, an exact lookup like the tints, on both sides.
  - Tints use `try_get_exact` and skip a line when the scope is unset.
    A fallback lookup lands on `diff.delta`'s text color and paints over syntax colors.
  - No bundled theme defines the `.view` scopes yet.
    Tints show once your theme adds them, or once #15631 lands its proposed defaults.
- Toggle is per view: a `toggle_inline_diff` static command.
  No default keybinding and no config default, so it's opt-in.
- `:diff-yank-base` yanks the base text of hunks under the selection.
  It's a sibling of `:reset-diff-change`. It exists because the cursor can't enter virtual rows.

Known limits:

- Virtual rows can't be selected or edited. Only the new side is editable. Old-side editing is what #15631 is for.
- Removed rows for a hunk that starts at line 0 can't render above line 0, because `LineAnnotation` only anchors after a line end. #15631 lists the same limit.
  With rows above the new lines, this hits every hunk at line 0, modifications included. Phase 1 renders them after line 0, marked `↑` in the diff gutter.
- Mouse clicks below virtual rows need coordinate checks. #15631 reports click offset bugs with filler lines. `text_pos_at_screen_coords` takes the annotations, so this may just work. Phase 1 didn't verify it.

Found in phase 1:

- A tinted line covers the cursorline.
  The decoration comes after the cursorline because its rows must follow inline diagnostics, the same order as the annotations.
- Removed lines after a last line with no trailing newline show no rows, because rows only follow a line ending.
- The annotation and the decoration each copy the diff.
  If the diff lands between the two copies, that frame is off, and the redraw the diff worker requests fixes it.

Found in phase 2:

- Changed words on the document side can't come from the decoration.
  A decoration is called by char index, and an inlay hint has the same char index as the real char after it.
  So the char behind a parameter hint would lose its highlight.
  The overlay cost a second seam in `render_view`: the `InlineDiff` is built before the overlays, and its overlay is pushed after the diagnostics.
- The base parse is cached on the view, so `View::inline_diff` became an `InlineDiffState`. Two views of one file parse its base twice.
- A `Rope` has no identity to compare.
  So each frame that needs syntax compares the cached base with the diff's, one pass over the base. The pass is cheap next to the frame, even for a large file.
- Word diffs are recomputed each frame, for the modified hunks that can be in view. Hunks over 16 KiB get none.
  There's no similarity cut-off, so a rewritten line has every word marked.
- `:diff-yank-base` yanks to the default yank register, like `y`, or to the register you name.
  `:yank-diagnostic` defaults to the clipboard instead.
  It picks hunks the way `:reset-diff-change` does, so a cursor on the line just after a hunk picks that hunk too.
- `imara-diff` is now a direct `helix-term` dependency. Bump it together with helix-core's.

### 5.2 Review session (phase 3)

- `:review [unstaged|staged|all]` builds an ordered file list from the fork's status walk (`for_each_changed_file`).
  It opens the first file with inline diff on.
  The changed-files picker gets a key to start a review from its current list.
- `review_next_file` / `review_prev_file`.
  `review_next_hunk` / `review_prev_hunk` cross into the next file at the last hunk.
  Users bind them in config (for example over `]g`/`[g`). `goto_next_change` stays untouched, so there's no seam in its body.
- Refresh the diff base (`Document::set_diff_base`) at session start and on each file switch, since the base only loads on open and reload.
- A sidebar is a picker in Helix, not a pane. The changed-files picker with preview plays that role. A persistent panel is out of scope.
- Optional: a statusline element ("review 3/12"). That's one enum variant plus one match arm.

### 5.3 Diff base and staging (phase 4)

- Reviewing only unstaged changes, and staging at all, needs the index as the base.
- Read the index blob in a child module, `helix-vcs/src/git/index.rs`.
  As a child it can call `git.rs`'s private `open_repo`, the same trick as `typed/yank_diagnostics.rs`.
- That costs one line in each of `git.rs` and `lib.rs`, which matters while #9951 and #16252 are open:
  - `mod index;` in `git.rs`.
  - `#[cfg(feature = "git")] pub use git::index;` in `lib.rs`, because `mod git` is private and feature-gated.
  - Callers in `helix-term` go behind `#[cfg(feature = "git")]` too.
- Stage a hunk by building a zero-context patch and running `git apply --cached --unidiff-zero`. Unstage with `-R` against `HEAD`→index.
  - Shelling out gets filters, CRLF, and index locking right for free.
  - Gate it on workspace trust: repo-local config like `core.fsmonitor` can run programs.
  - A pure-gix version (write the blob, update the index entry) is possible later, but you'd have to reimplement clean filters.
- Upstream reads diffs with gix only and never needs the `git` binary. That's a fork-only dependency, so record it in the patch ledger.
  The fork has no patch ledger yet. Create one before phase 4.

### 5.4 Agent notes (optional, zero Helix code)

- A tiny external language server reads a notes sidecar and publishes `Hint`/`Information` diagnostics.
  Add it as an extra `language-servers` entry.
- What you get for free: inline rendering, `]d`, the diagnostics picker, and the fork's `:yank-diagnostics`.
- Hunk's notes go through its daemon (`hunk session`). Interop isn't worth it. Agents can write the sidecar directly.
- A native version would be one more `LineAnnotation` at the same seam as 5.1. Do that only if the LSP route falls short.

## 6. Hunk feature map

| Hunk | Helix-native equivalent | Status |
| --- | --- | --- |
| `[` `]` hunks | `[g` `]g`, plus cross-file `review_*_hunk` | Exists / phase 3 |
| Stack (unified) layout | Inline overlay | Done (phase 1) |
| Split layout | #15631 | Upstream |
| Multi-file stream + sidebar | Review session + changed-files picker | Phase 3 (not one continuous scroll) |
| Syntax highlighting | New side: already there. Removed rows: base parse | Done (phase 2) |
| Word-level highlight (not listed for Hunk) | Token diff overlay | Done (phase 2) |
| Agent notes inline | Notes LSP → diagnostics | Optional |
| Watch mode | None. Base refresh on session navigation only | Partial |
| Pager / difftool | `git diff \| hx` | Exists |
| Edit while reviewing | Real buffer | Done (phase 1) |
| Stage / unstage hunk | `git apply --cached` | Phase 4 |
| jj / Sapling | Gix only. jj colocated repos work through git | Upstream #12022 |

## 7. Fork fit: seams and new files

Seams (one line each, marked `// FORK:`):

| File | Commits in last year | Seam |
| --- | --- | --- |
| `helix-view/src/view.rs` | 3 | Field `inline_diff: InlineDiffState` on `View` and its default in `View::new`. One call in `text_annotations`. |
| `helix-term/src/ui/editor.rs` | 9 | In `render_view`: build `InlineDiff` and push its word overlay before the selection overlays, then one `add_decoration` call. |
| `helix-term/src/commands.rs` | 39 | Static command rows plus one-line wrappers, as `changed_file_picker` does. |
| `helix-term/src/commands/typed.rs` | 20 | `TYPABLE_COMMAND_LIST` rows, and a `mod` line for each body in `typed/<name>.rs`. |
| `helix-term/Cargo.toml` | 28 | `imara-diff`, after `parking_lot.workspace`, which version bumps don't touch |
| `helix-view/src/annotations.rs`, `helix-term/src/ui/text_decorations.rs` | 1 | `mod diff;`, plus a re-export in `text_decorations.rs` |
| `book/src/themes.md` | 8 | `view` under `diff.{plus,minus,delta}`, `text` under `diff.delta.view` |
| `helix-term/src/commands/changed_file.rs` | fork-owned | "Start review" picker key |
| `helix-term/tests/integration.rs` | 1 | `mod fork_diff;` in the `mod test` block |
| `helix-view/src/editor.rs` | 16 | Only for a config field or statusline variant. Phase 1 needs none. |
| `helix-vcs/src/git.rs` | 3, under rewrite in open PRs | Phase 4: `mod index;` |
| `helix-vcs/src/lib.rs` | 1, under rewrite in open PRs | Phase 4: `pub use git::index;` behind `cfg(feature = "git")` |

New fork-owned files:

- `helix-view/src/annotations/diff.rs`: hunk → virtual-row mapping. Pure functions, unit-tested.
- `helix-term/src/ui/text_decorations/diff.rs`: painting removed rows, tints, word highlights.
- `helix-term/src/ui/text_decorations/diff/words.rs`: the word diff of a hunk. Pure, unit-tested.
- `helix-term/src/commands/review.rs`: toggle, the review session, and file and hunk navigation.
- `helix-term/src/commands/typed/{review,diff_yank_base,stage_hunk}.rs`
- `helix-vcs/src/git/index.rs`: index base and the staging shell-out.
- `helix-term/tests/test/fork_diff.rs`: integration tests. Assert editor state: selection after `review_next_file`, register contents after `:diff-yank-base`, index contents after staging.

Avoid `helix-view/src/document.rs`.
It has 16 upstream commits in the last year, as many as `editor.rs`, and it isn't on the `AGENTS.md` hot list.
The public `set_diff_base` and `diff_handle` are enough.

## 8. Phases

| Phase | Status | Scope | Fork-owned lines (rough) | Risk |
| --- | --- | --- | --- | --- |
| 1. Inline MVP | Done | Per-view toggle. Removed rows above the new lines, truncated, no syntax. Tints where the theme defines `.view` scopes. Gutter markers on removed rows. | 400–600. Actual: about 410, plus 300 of tests. | Low. Row math is the only tricky part, and it's unit-testable. Copy out of the diff lock (5.1). |
| 2. Polish | Done | Soft-wrap heights, word-level highlights, base syntax highlights, `:diff-yank-base` | 400–600. Actual: about 540, plus 430 of tests. | Medium: caching the base `Syntax` and wrap math |
| 3. Review session | Next | `:review`, file and cross-file hunk navigation, picker entry, base refresh | 300–500 | Low |
| 4. Index base + staging | Planned | `git/index.rs`, `:stage-hunk`, `:unstage-hunk`, trust gate | 300–500 | Medium: upstream diff-base rewrite, `git` dependency |
| — Side-by-side | Waiting on #15631 | Rebase onto #15631 once merged | 0 | Its theme scopes and `]g` changes may need reconciling with phase 1 |

For scale: #15631's core file alone is 2038 lines.

## 9. Open questions for you

Decided: removed rows go above the new lines.

These gate phases 3–4, not phase 1:

- Default base for a review: `HEAD` (everything an agent did since the last commit) or the index (only what isn't staged)? `HEAD` matches today's gutter, so it's simpler.
- Is staging in scope, and is a `git` binary dependency acceptable for it?

## Side findings

- `helix-view/src/document.rs` churns like `editor.rs` (16/yr). Consider adding it to the `AGENTS.md` hot-file list.
- After a commit, gutters stay stale until `:reload-all`. Phase 3's base refresh fixes that for files under review. A general fix would be a seam in `document.rs`. Skip it.
- `TextRenderer::draw_decoration_grapheme` checks the row against the viewport height before subtracting the vertical offset.
  When the view starts partway into a wrapped line, inline diagnostics near the bottom seem to be dropped. Phase 2 draws its rows without it.

## Sources

- [Hunk site](https://www.hunk.dev/), [modem-dev/hunk README](https://github.com/modem-dev/hunk)
- [helix#15631](https://github.com/helix-editor/helix/pull/15631), [#9951](https://github.com/helix-editor/helix/pull/9951), [#16252](https://github.com/helix-editor/helix/pull/16252), [#15938](https://github.com/helix-editor/helix/pull/15938), [#12022](https://github.com/helix-editor/helix/pull/12022), [#14434](https://github.com/helix-editor/helix/pull/14434)
- [helix#405](https://github.com/helix-editor/helix/issues/405), [#9127](https://github.com/helix-editor/helix/issues/9127), [#5166](https://github.com/helix-editor/helix/issues/5166), [#6101](https://github.com/helix-editor/helix/issues/6101), [#13383](https://github.com/helix-editor/helix/issues/13383), [#15611](https://github.com/helix-editor/helix/issues/15611)
