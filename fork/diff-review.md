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
- **Staging hunks** came in phase 4, through the `git` binary.
  It needs the index as a diff base, and upstream is rewriting exactly that plumbing right now, so the fork's VCS seams stay one line each.
- **Size:** phases 1–4 came to about 1.8k fork-owned lines, against an estimate of 1.4–2.2k. About a dozen one-line seams.
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

- `:review [unstaged|staged|all]` lists the changes of the repository that holds the current file, or of the working directory for a buffer with no path.
  `all` is the default.
  - The list comes from the fork's status walk (`for_each_changed_file`) in the background. A job callback starts the session when the walk ends.
  - Files go in path order. The first one opens with inline diff on, at its first hunk.
- In the changed-files picker, `start_review` (`Alt-r`) reviews the files the picker shows, in the order shown, from the selected one.
  A query narrows the review.
- `review_next_file` / `review_prev_file` open the neighboring file at its first hunk. A count skips files.
- `review_next_hunk` / `review_prev_hunk` step like `]g`/`[g`.
  Past the last hunk of the review's current file, they open the next file at its first hunk. Before the first hunk, they open the previous file at its last hunk.
  - They only cross files from the file the review is on. Anywhere else they stay in the file, like `]g`.
    So binding them over `]g`/`[g` changes nothing outside a review.
  - `goto_next_change` stays untouched, so there's no seam in its body.
- Each file switch refreshes the diff base (`Document::set_diff_base`) from `HEAD`, since the base only loads on open and reload.
  A commit during a review no longer leaves stale hunks.
- Every file diffs against `HEAD`. The list only picks files, so `unstaged` still shows a file's staged hunks. The index base is phase 4.
- A file with no `HEAD` version, like a new or untracked one, gets an empty base, so its text is one added hunk.
  A deleted file opens as an empty buffer, with its old text as removed rows. A renamed file diffs against its old path.
- Each switch says `Review 3/12: path`, plus `(new)`, `(deleted)`, `(conflict)`, or `(renamed from …)` when the text doesn't show it.
  There's no statusline element. One would cost a `StatusLineElement` variant in `editor.rs`.
- A sidebar is a picker in Helix, not a pane. The changed-files picker with preview plays that role. A persistent panel is out of scope.

Found in phase 3:

- The session lives on the view, like the inline diff toggle. That costs a field in `view.rs` and no seam in `editor.rs`. A split starts without a session.
  The type is in a new `helix-view/src/review.rs`, which costs `pub mod review;` in `helix-view/src/lib.rs`.
- The diff worker runs in the background, and a base update waits out its 96 ms debounce. So a file switch can't land on a hunk from the worker.
  `open_file` diffs the file itself with the worker's settings (Histogram, the indent heuristic, the size limits) to pick the hunk.
  Steps within a file use the worker's hunks, like `]g`. Right after a switch that changed the base, they can lag by that debounce.
- The session keeps an index, like a quickfix list. Opening another file doesn't move it, and `review_next_file` continues from the index.
  A file that fails to open still takes the index, so the next step moves past it.
- Hunk steps use the primary selection and leave one selection. Unlike `]g`, they don't extend in select mode.
- `:review` walks the current file's repository, but the changed-files picker walks the working directory. They differ only when the current file is in another repository.
  Integration tests can't change the working directory, so the picker key has no end-to-end test. It was checked by hand in a pty.
- `:review` is in `commands/review.rs` next to the session, not in `typed/review.rs`. It calls no `typed.rs` helper, so `typed.rs` only needs its row.
- In the test harness, `event_loop_until_idle` clears the idle timer when it returns. A later run that gets no events never returns, so the tests' wait helper re-arms the timer.

### 5.3 Diff base and staging (phase 4)

Decided 2026-10-15: a review diffs against `HEAD` by default, staging is in scope, and it runs the `git` binary.

- `:review` keeps `all` and `HEAD` as its default, which matches the gutter in every other file.
- `:review unstaged` diffs against the index, so it shows only what isn't staged. Each file switch loads the scope's base.
  - With nothing staged, the index and `HEAD` give the same hunks.
  - The base belongs to the document, not the view. An `unstaged` review changes the file's gutter in every view, until a reload or a switch back to `HEAD`.
- `:review staged` stays a file filter over the `HEAD` diff. A buffer holds worktree text, so the `HEAD`→index diff can't be drawn on it.
- Read the index blob in a child module, `helix-vcs/src/git/index.rs`, and convert it to worktree text the way `get_diff_base` does.
  As a child it can call `git.rs`'s private `open_repo`, the same trick as `typed/yank_diagnostics.rs`.
- That costs one line in each of `git.rs` and `lib.rs`, which matters while #9951 and #16252 are open:
  - `pub(crate) mod index;` in `git.rs`.
  - `pub mod index;` in `lib.rs`, for a facade in `helix-vcs/src/index.rs`. `mod git` is private and feature-gated.
- `:stage-hunk` diffs the index text against the buffer and builds the file's new index text, with the hunks under the selection taken from the buffer.
  It pipes that to `git hash-object -w --stdin --path=<path>`, then runs `git update-index --cacheinfo <mode>,<oid>,<path>`.
  - `--path` applies the file's attributes on the way in, so CRLF conversion and clean filters come out right.
  - Not `git apply --cached`: it writes patch text to the index unconverted, and the base and the buffer are worktree text.
    In a scratch repo with `*.txt text eol=crlf`, a zero-context patch built from worktree text failed with "patch does not apply". The `hash-object` route stored LF.
  - `:unstage-hunk` writes `HEAD`'s text back for the `HEAD`→index hunks on the `HEAD` lines under the selection.
  - In an `all` review, a staged hunk stays on screen, since it still differs from `HEAD`. The command reports what it staged.
  - Both refuse in an untrusted workspace (`is_trusted()`). Repo-local config like `core.fsmonitor` and clean filters can run programs.
- A pure-gix version is possible later. gix 0.85 has `Pipeline::convert_to_git`, and gix-index can write the index.
  The cost is the fork writing `.git/index` itself: the lock, and extensions gix may not keep.
- Upstream reads diffs with gix only and never needs the `git` binary. The `diff-review` entry in `FORK.md` records the requirement.

Found in phase 4:

- The plan put `helix-term`'s callers behind `#[cfg(feature = "git")]`.
  The facade in `helix-vcs/src/index.rs` errors without the feature instead, like the diff providers, so `helix-term` has no `cfg`.
- Both commands read the index and `HEAD` again and diff them against the buffer themselves.
  So they don't depend on the view's base or wait for the diff worker.
- `:unstage-hunk` maps the selections' lines to `HEAD` lines through the `HEAD`→buffer diff.
  So it also finds staged hunks that aren't on screen, as in an `unstaged` review, and lines where the buffer went back to `HEAD`'s text.
- Staging takes the buffer's text, unsaved edits included.
- Staging all of a deleted file removes it from the index. Unstaging all of a file `HEAD` doesn't have removes it too.
- `git` runs with the `--git-dir` and `--work-tree` gix found, and without `GIT_INDEX_FILE`, because gix reads `<git-dir>/index`.
- Staging refuses a conflicted file. An `unstaged` review diffs a conflicted file against `HEAD`, since the index has no unconflicted version.
- After staging, the base reloads only when the focused view reviews the file against the index.
  A document that an earlier `unstaged` review left on the index base stays stale until the review opens it again.
- The changed-files picker's `start_review` follows the list it shows, so the unstaged list (its default) reviews against the index.
  Checked by hand in a pty, through `:diff-yank-base` on the first hunk: the unstaged list yanked the index text, the `all` list `HEAD`'s.
- A new file is staged executable when it is executable on disk, unless `core.fileMode` is off.
  The staged text keeps the index version's byte order mark.
- Each command runs `git` twice on the main thread. In a large repository `update-index` rewrites the whole index, which can take a moment.

### 5.4 Agent notes (optional, zero Helix code)

- A tiny external language server reads a notes sidecar and publishes `Hint`/`Information` diagnostics.
  Add it as an extra `language-servers` entry.
- What you get for free: inline rendering, `]d`, the diagnostics picker, and the fork's `:yank-diagnostics`.
- Hunk's notes go through its daemon (`hunk session`). Interop isn't worth it. Agents can write the sidecar directly.
- A native version would be one more `LineAnnotation` at the same seam as 5.1. Do that only if the LSP route falls short.

## 6. Hunk feature map

| Hunk | Helix-native equivalent | Status |
| --- | --- | --- |
| `[` `]` hunks | `[g` `]g`, plus cross-file `review_*_hunk` | Exists / Done (phase 3) |
| Stack (unified) layout | Inline overlay | Done (phase 1) |
| Split layout | #15631 | Upstream |
| Multi-file stream + sidebar | Review session + changed-files picker | Done (phase 3), not one continuous scroll |
| Syntax highlighting | New side: already there. Removed rows: base parse | Done (phase 2) |
| Word-level highlight (not listed for Hunk) | Token diff overlay | Done (phase 2) |
| Agent notes inline | Notes LSP → diagnostics | Optional |
| Watch mode | None. Base refresh on session navigation only | Partial |
| Pager / difftool | `git diff \| hx` | Exists |
| Edit while reviewing | Real buffer | Done (phase 1) |
| Stage / unstage hunk | `:stage-hunk`, `:unstage-hunk` through `git hash-object --path` + `git update-index` | Done (phase 4) |
| jj / Sapling | Gix only. jj colocated repos work through git | Upstream #12022 |

## 7. Fork fit: seams and new files

Seams (one line each, marked `// FORK:`):

| File | Commits in last year | Seam |
| --- | --- | --- |
| `helix-view/src/view.rs` | 3 | Fields `inline_diff: InlineDiffState` and `review: Option<ReviewSession>` on `View`, and their defaults in `View::new`. One call in `text_annotations`. |
| `helix-view/src/lib.rs` | 3 | `pub mod review;` |
| `helix-term/src/ui/editor.rs` | 9 | In `render_view`: build `InlineDiff` and push its word overlay before the selection overlays, then one `add_decoration` call. |
| `helix-term/src/commands.rs` | 39 | Static command rows plus one-line wrappers, as `changed_file_picker` does. |
| `helix-term/src/commands/typed.rs` | 20 | `TYPABLE_COMMAND_LIST` rows, and a `mod` line for each body in `typed/<name>.rs`. `:review`'s row points into `commands/review.rs`, so it has no `mod` line. |
| `helix-term/Cargo.toml` | 28 | `imara-diff`, after `parking_lot.workspace`, which version bumps don't touch |
| `helix-view/src/annotations.rs`, `helix-term/src/ui/text_decorations.rs` | 1 | `mod diff;`, plus a re-export in `text_decorations.rs` |
| `book/src/themes.md` | 8 | `view` under `diff.{plus,minus,delta}`, `text` under `diff.delta.view` |
| `helix-term/src/commands/changed_file.rs` | fork-owned | The `with_on_start_review` call |
| `helix-term/src/ui/picker.rs` | 3 | The `on_start_review` hook and its key arm, next to the fork's `on_cycle_changes` |
| `helix-view/src/editor/picker_keys.rs` | fork-owned | `StartReview`, bound to `Alt-r` |
| `book/src/keymap.md` | 5 | `start_review` in the picker table |
| `helix-term/tests/integration.rs` | 1 | `mod fork_diff;` in the `mod test` block. Review tests are a child module of `fork_diff.rs`. |
| `helix-view/src/editor.rs` | 16 | Only for a config field or statusline variant. Phase 1 needs none. |
| `helix-vcs/src/git.rs` | 3, under rewrite in open PRs | `pub(crate) mod index;` |
| `helix-vcs/src/lib.rs` | 1, under rewrite in open PRs | `pub mod index;` |

New fork-owned files:

- `helix-view/src/annotations/diff.rs`: hunk → virtual-row mapping. Pure functions, unit-tested.
- `helix-term/src/ui/text_decorations/diff.rs`: painting removed rows, tints, word highlights.
- `helix-term/src/ui/text_decorations/diff/words.rs`: the word diff of a hunk. Pure, unit-tested.
- `helix-view/src/review.rs`: the review session a view holds.
- `helix-term/src/commands/review.rs`: toggle, `:review`, the review session, and file and hunk navigation.
- `helix-term/src/commands/typed/{diff_yank_base,stage_hunk}.rs`
- `helix-vcs/src/git/index.rs`: index base and the staging shell-out.
- `helix-vcs/src/index.rs`: the facade over `git/index.rs`, which errors without the `git` feature.
- `helix-term/tests/test/fork_diff.rs`: integration tests. Assert editor state: selection after `review_next_file`, register contents after `:diff-yank-base`, index contents after staging.
  `fork_diff/review.rs` runs review sessions over a temporary git repository, and its child `fork_diff/review/stage.rs` stages in one.

Avoid `helix-view/src/document.rs`.
It has 16 upstream commits in the last year, as many as `editor.rs`, and it isn't on the `AGENTS.md` hot list.
The public `set_diff_base` and `diff_handle` are enough.

## 8. Phases

| Phase | Status | Scope | Fork-owned lines (rough) | Risk |
| --- | --- | --- | --- | --- |
| 1. Inline MVP | Done | Per-view toggle. Removed rows above the new lines, truncated, no syntax. Tints where the theme defines `.view` scopes. Gutter markers on removed rows. | 400–600. Actual: about 410, plus 300 of tests. | Low. Row math is the only tricky part, and it's unit-testable. Copy out of the diff lock (5.1). |
| 2. Polish | Done | Soft-wrap heights, word-level highlights, base syntax highlights, `:diff-yank-base` | 400–600. Actual: about 540, plus 430 of tests. | Medium: caching the base `Syntax` and wrap math |
| 3. Review session | Done | `:review`, file and cross-file hunk navigation, picker entry, base refresh | 300–500. Actual: about 380, plus 420 of tests. | Low |
| 4. Index base + staging | Done | `git/index.rs`, index base for `:review unstaged`, `:stage-hunk`, `:unstage-hunk`, trust gate | 300–500. Actual: about 500, plus 360 of tests. | Medium: upstream diff-base rewrite, `git` dependency |
| — Side-by-side | Waiting on #15631 | Rebase onto #15631 once merged | 0 | Its theme scopes and `]g` changes may need reconciling with phase 1 |

For scale: #15631's core file alone is 2038 lines.

## 9. Decisions

- Removed rows go above the new lines.
- A review diffs against `HEAD` by default, the same as the gutter. `:review unstaged` uses the index. (2026-10-15)
- Staging is in scope. It runs the `git` binary and only works in a trusted workspace. (2026-10-15)
- The patch ledger lives in `FORK.md`. (2026-10-15)

## Side findings

- `helix-view/src/document.rs` churns like `editor.rs` (16/yr). Consider adding it to the `AGENTS.md` hot-file list.
- After a commit, gutters stay stale until `:reload-all`. Phase 3's base refresh fixes that for files under review. A general fix would be a seam in `document.rs`. Skip it.
- Clippy's `type_complexity` flags the fork's `on_cycle_changes` field in `ui/picker.rs`. Phase 3 gave its own hook a type alias. The older field still warns.
- `TextRenderer::draw_decoration_grapheme` checks the row against the viewport height before subtracting the vertical offset.
  When the view starts partway into a wrapped line, inline diagnostics near the bottom seem to be dropped. Phase 2 draws its rows without it.

## Sources

- [Hunk site](https://www.hunk.dev/), [modem-dev/hunk README](https://github.com/modem-dev/hunk)
- [helix#15631](https://github.com/helix-editor/helix/pull/15631), [#9951](https://github.com/helix-editor/helix/pull/9951), [#16252](https://github.com/helix-editor/helix/pull/16252), [#15938](https://github.com/helix-editor/helix/pull/15938), [#12022](https://github.com/helix-editor/helix/pull/12022), [#14434](https://github.com/helix-editor/helix/pull/14434)
- [helix#405](https://github.com/helix-editor/helix/issues/405), [#9127](https://github.com/helix-editor/helix/issues/9127), [#5166](https://github.com/helix-editor/helix/issues/5166), [#6101](https://github.com/helix-editor/helix/issues/6101), [#13383](https://github.com/helix-editor/helix/issues/13383), [#15611](https://github.com/helix-editor/helix/issues/15611)
