# Helix fork: a review stream

Snapshot: fork branch `fork` at `81f68636`, on upstream `master` at `ba40e547`.
Reference views: Hunk's unified layout ([hunk.dev](https://www.hunk.dev/)), Zed's project diff.
Supersedes the diff review plan. Its code never reached the rebuilt `fork`.
Its plan and code stay under the local tag `archive/diff-review` (6.1).

Status: phase 5 done 2026-10-17. Phase 6 follows without a stop, as the human asked.

## Bottom line

- **Build:** one buffer that shows every changed file, Hunk style.
  - Only the hunks and their context show. The unchanged code between them collapses to a row.
  - Every row is a real line of the buffer, so motions, selections, `/`, and yank work across all of it.
  - It starts read-only. `enter` opens the file at that line, and you edit there, with LSP. `C-o` comes back.
- **Then, if it works out, editing in the stream.** Added and context lines become editable, and an edit lands in the file's own buffer at once.
  It's the last milestone. Dropping its commits leaves a working read-only review.
- **How:** the stream is an ordinary Helix document. Fork-owned code builds it and keeps it current through upstream's events and public API.
- **No hard fork.** The read-only review costs about 15–20 one-line seams, plus command-table rows. Editing adds about 3.
  None is in `document.rs` or `application.rs`. `ui/editor.rs` takes the most, about 5–7.
- **Size:** about 2.5–4k fork-owned lines for the read-only review, plus tests. Editing adds 1.1–1.7k. LSP in the stream is unknown until a spike.
- **Biggest risks:** keeping the cursor still while rows appear and vanish around it (phase 2, which the read-only review already needs).
  Then, for editing, the sync engine, undo across files, and completion.

## 1. The view

```
 helix-term/src/ui/editor.rs  modified                             header row
          ⋯ 40 unchanged lines                                     gap row
  41  41    fn render(&self, area: Rect) {                          context
  42     -      let width = area.width - 2;                         removed
      42 +      let width = area.width.saturating_sub(2);           added
  43  43        let height = area.height;
          ⋯ 120 unchanged lines
 helix-view/src/review.rs  new file
       1 +  pub struct ReviewStream {
```

The gutter shows the old line number, the new line number, and the sign. The text has no `+` or `-`, so yanked code comes out clean.
A header's text is the path and the status. The rule around it is a decoration (phase 4).

| Row | Text | Editable from phase 7 | Maps to |
| --- | --- | --- | --- |
| Header | path and status | no | the file |
| Gap | `⋯ N unchanged lines` | no | a hidden range of source lines |
| Removed | base line | no | a base line |
| Context | source line | yes | a base line and a source line |
| Added | source line | yes | a source line |

Every row is exactly one line of the stream document. Removed rows go above the added rows, as in git and Hunk.

What works because the stream is a document: motions, `/` across every file's hunks, selections, multiple cursors across files, registers, macros, splits.

## 2. Why a buffer

Three other shapes were weighed.

- **A pane, like the file picker.** Opened from space mode, as an overlay, with `enter` jumping to the file.
  - It would need no render seams, and it could draw Hunk's split layout.
  - Rejected: Helix commands act on the focused view in the editor's tree. `current!` reads `editor.tree.focus` (`helix-view/src/macros.rs:13`).
    An overlay isn't in the tree, so motions, select mode, `/`, and yank would each be rewritten on `helix-core`, and wouldn't follow the user's keymap.
    Read-only editor features inside hunks are a requirement.
- **An inline diff on each file**, as the diff review plan built. It walked files one at a time, not as one stream.
- **A multibuffer.** The diff review plan rejected one (its option D) because a view shows one document, and because the prototype put most of its code in hot files.
  This design keeps one document per view. The stream is that document.
  The many-files part lives in fork-owned code that copies text from the file buffers into the stream, and, once editing exists, back.
  It runs on upstream's public API (`Document::apply`, `history`, `reset_modified`, `indent_style`, `line_ending`, `replace_diagnostics`) and on its events (`DocumentDidChange`, `PostCommand`, `PostInsertChar`, `OnModeSwitch`, `DiagnosticsDidChange`, `DocumentDidClose`).

Prior art:

- matoous's `md/multibuffer` branch (#15611) has the same shape: one document holding the excerpts.
  - It writes back to the files on `:w`, not as you type.
  - It puts about 850 lines into `view/editor.rs` (+588) and `document.rs` (+259).
  - It changes `helix-core` (`text_annotations.rs`, `doc_formatter.rs`) to draw headers as virtual lines above line 0.
    Here headers are real lines, so the line-0 limit doesn't come up.
  - It parses each excerpt on its own. A hunk that starts inside a string highlights wrong. Here highlights come from full-file parses (3.5).
- Zed's project diff is editable, and the cursor moves through deleted regions as through any text.
  Zed's multibuffer history groups one transaction per buffer (`buffer_transactions` in `crates/multi_buffer/src/transaction.rs`). The stream's undo log (3.9) has the same shape.
  Zed can undo one buffer's transaction out of order, because its buffers are CRDTs. Helix's history can't.

## 3. Design

### 3.1 The stream document

- A scratch document: no path, no language, no language servers.
- Its text is the rows, one per line. A source row keeps the file's line ending, so stream and source chars match one to one inside a row.
- The row table lives on the editor, in a fork type: `Editor::review`.
  It needs other documents and is read where `&Editor` is at hand: gutter, render, commands.
- A **run** is a maximal stretch of consecutive editable rows. A run maps to one contiguous range of source text.
  The two runs on either side of a removed block are adjacent in the source. Runs matter once editing exists (3.9).
- Headers and gaps are real lines.
  The cursor can stop on them, and `ret` on them does something useful (3.6).
  Virtual lines can't go above line 0 without a `helix-core` change, which the prototype needed.
- Files come in path order. Context is 3 lines, as in git.
  Hunks whose context touches merge, as in git. A gap row appears only when lines are hidden.
- One stream per editor. `:review` with a stream open switches to it. `:review-refresh` rebuilds it from git.
  Several views of it work like views of any document.

### 3.2 Bases and sources

- Each file has a base: the `HEAD` blob, or the index blob in an `unstaged` review, converted to worktree text.
  `HEAD` comes from upstream's `DiffProviderRegistry::get_diff_base`. The index needs `git/index.rs` (phase 6).
- Each file has a source: the file's document if one is open, or the text on disk.
  The read-only review never opens a document for a file. Editing does (3.9).
- The stream computes hunks itself, with `imara-diff`, Histogram, and upstream's size caps (`MAX_DIFF_LINES`, `MAX_DIFF_BYTES`).
  The diff worker's hunks lag by its 96 ms debounce, and the layout has to be right before the next key.
  A file over the caps gets a header and no rows.

### 3.3 Read-only (phase 1)

- A check at the start of `EditorView::render`, `review::sync`, runs before every frame.
  It sees an edit from any source before it's drawn: a key, an on-next-key callback like `r`, a typable command, a paste.
- The stream keeps a copy of the text it has to hold, and the buffer's version when it last held it.
  When the version moved, the check puts the text back with one transaction, restores each view's selection from the last frame, and says why in the status line.
- In insert mode in the stream, the check calls `Editor::enter_normal_mode` first, since leaving insert mode can edit the buffer.
- After a restore, the stream's history is cleared and `reset_modified` is called, so `u` finds nothing and the stream never counts as modified.
  Every view syncs its jumps before the history goes, then records revision 0 (6.2).
- The buffer is `readonly`, so the statusline says so. Upstream only uses that flag for writes.
- `:w` in the stream fails, as for any buffer with no path.

### 3.4 Keeping it current (phase 2)

- The check at the start of `render` compares each open source's version with the one its rows came from, and re-diffs and reconciles the files that moved.
  A `DocumentDidChange` hook isn't needed for that.
- **Reconcile** builds the rows a file should have, compares them with its current rows by identity, and applies one transaction to the stream:
  insert and delete rows, and copy source text into rows whose text changed.
  Selections map through it like through any edit, so the cursor stays on its row, in every view of the stream.
- **Identity, not text.** Rows are matched by file and `Key`: `Header`, `Gap(first hidden source line)`, `Removed(base line)`, and `Source(source line)` for context and added rows.
  The old rows' lines go through a line map of the old text to the new first (6.3).
  A text diff (`helix_core::diff::compare_ropes`) matches equal lines, and diff output often has neighboring lines that are equal (a `}` removed above a `}` kept).
  Matching by text could leave the cursor on the removed row instead of the line it was on.
- So an edit made after `enter` shows in the stream by the time `C-o` brings you back.
- A file that isn't open changes only on disk, and there's no file watcher in this tree. `:review-refresh` picks it up.
- Showing a gap's hidden lines is a relayout too, so it comes here.

### 3.5 Rendering

- **Gutter (phase 1).** `gutter.rs`'s `line_numbers` and `diff` take a stream branch: old and new numbers, then the sign.
  No new `GutterType`, so no change to the enum in `editor.rs` or to config parsing.
  The width comes from the view's `gutters.line_numbers.min_width`, which the check sets on the stream's views (6.2).
  The sign column uses `diff.plus` and `diff.minus`, so every theme tells the rows apart.
- **Row tints (phase 4)** use #15631's scopes, `diff.{plus,minus,delta}.view`, as diff review did.
- **Word highlights (phase 4)** reuse `words.rs` from `archive/diff-review`.
- **Header and gap rows (phase 4)** get a decoration. Their styles fall back to existing UI scopes, so bundled themes work. Phase 4 names the scopes.
- **Syntax (phase 5).**
  - The new side uses the source document's own `Syntax` when the file is open. A loaded document is parsed already, and it updates as you type.
  - The text on disk and the old side are parsed on the blocking pool and cached per file, as diff review's phase 2 did.
  - The highlights of rows on screen map into stream positions and go in as overlays.
  - The document highlighter is off for the stream. That's one seam in `render_view`.

### 3.6 Keys

The stream gets its own keymap: your keymap with the stream's bindings over it.
`EditorView` swaps it in while a stream has focus, so pending keys and sticky modes stay per keymap. That costs a field and a seam, in phase 1.
The stream's bindings replace commands, not keys, so your own bindings for `goto_next_change` and the others carry over.

Draft:

| Key | In the stream | Upstream meaning | Phase |
| --- | --- | --- | --- |
| `ret` | Code row: open the file at that line. Header: open the file. Gap: open the file at the first hidden line, then from phase 2 show the hidden lines | unbound in normal mode | 1, 2 |
| `]g` `[g` | next / previous hunk, across files | next / previous change | 3 |
| `]G` `[G` | last / first hunk | last / first change | 3 |
| `]F` `[F` | next / previous file | unbound | 3 |
| `u` `U` `A-u` `A-U` | the stream log (3.9) | the document history | 8 |

`ret` records a jump first, so `C-o` returns to the stream. `commands/review.rs` is a child of `commands`, so it can call the private `push_jump`.

Typable: `:review [all|unstaged|staged]`, `:review-refresh`, `:review-context <n>`, `:stage-hunk`, `:unstage-hunk`, `:review-restore`.
`:review-restore` writes the base text over the hunks under the selection into the source, like `:reset-diff-change`, and the refresh removes them from the stream.
It opens the file's document if it isn't open, so the restore can be undone there.

### 3.7 Scopes and file kinds

- `:review` and `:review all` diff against `HEAD`, like the gutter. `:review unstaged` diffs against the index (phase 6).
- `:review staged` shows `HEAD` → index (phase 6). Its new side is the index, not a file, so it stays read-only after phase 7.
- A new or untracked file has an empty base, so all its rows are added.
- A deleted file has only removed rows. `:review-restore` brings it back.
- A renamed file diffs against its old path.
- A conflicted file diffs against `HEAD`, as diff review's phase 4 ruled, and says `conflict` in its header.
- A binary file, or one over the size caps, shows its header and no rows.

### 3.8 Staging (phase 6)

- Port diff review's phase 4 from `archive/diff-review`: `helix-vcs/src/git/index.rs`, the `helix-vcs/src/index.rs` facade, and the logic in `typed/stage_hunk.rs`.
  Its rulings stand: `git hash-object --path` and `git update-index`, the trust gate, refusing a conflicted file.
- Staging and unstaging work on the hunks or lines under the selection in the stream.
- An `unstaged` review reloads its bases after staging, so staged hunks leave the stream.
- The changed-files picker jumps into the stream at the file's header, instead of opening the file.

### 3.9 Editing (phases 7–8, if it works out)

Everything here builds on phase 2's reconcile. If phase 7's relayout feels wrong in use, its commits are dropped, and the read-only review stays.

**Loading.** The stream opens a file's document with `Action::Load` when an edit reaches its section.
`Action::Load` gives the document a selection for the current view and no view of its own.
A file that loads with text different from what the stream showed gets its section rebuilt, and an edit that raced it is refused.
Closing the stream closes the documents it loaded that are unmodified and shown in no view.

**Capture.** A `DocumentDidChange` hook queues `(doc, old_text, changes)` for the stream and its sources, in order.
The hook only gets `&mut Document`, so it can't touch the other side itself (the note at `document.rs:223`).

**Sync points**, each with `&mut Editor`:

- `PostCommand` and `PostInsertChar` hooks, after every command and every typed character. Macros sync between their keys.
- The start of `EditorView::handle_event` and `EditorView::render`. Seams.
  They catch edits that don't come from a command, without the frame of lag a dispatched callback has: typable commands, bracketed paste, LSP edits, and job callbacks.

**A sync:**

1. **Check the stream's edits.** A change must fall inside one run.
   An insert at a run's start is allowed. The line break that ends a run can't be deleted, since the stream can't show the join.
   If any change breaks the rule, the whole window reverts, as in phase 1, and a status message says why.
2. **Forward.** Shift each change by its run's offset, and apply the result to the source document with the stream's view id.
3. **Map rows.** Move the row table through the stream's and the sources' changes.
4. **Reconcile** each touched file, as in phase 2.
5. **Tidy.**
   - Commit and clear the stream's own history, and `reset_modified`, so the stream never counts as modified.
   - Set the stream's `indent_style` and `line_ending` to those of the file under the primary cursor.

A sync window normally holds one command's changes.
If the stream and a source both changed in one window (rare: an LSP edit and a key, with no render between them), the sync replays the queue in order.
It shifts the source's changes past the stream's. If they overlap, the file's section is rebuilt from the source and the stream edit is refused.

**No echo.** Reconcile compares the rows a file should have with the rows it has.
A forwarded edit already made them equal, so it causes no second edit.

**Relayout is live, as in Zed** (decision 8.1).
Edit a context line, and its old version appears above it as a removed row. The line itself turns into an added row.
The new row is an insert before the cursor's line, so the cursor stays on the text it was on.
Change an added line back to the base text, and its removed twin vanishes.

**Undo and redo (phase 8).**

- The stream's own history is never used. Its revisions would mix your edits with relayouts.
- The stream keeps a log. Each entry is one stream command and lists the `(source document, revision after)` pairs it touched.
  A stream command is one normal-mode command, or one insert session ending at `Esc`.
  When it ends, the sync calls `append_changes_to_history` on each touched source.
- `undo` in the stream pops an entry and undoes each listed source whose current revision matches.
  If a source moved on since, for example you edited it in another split, the undo stops and names the file.
- `redo` is the same in reverse.
- The stream's keymap swaps `undo`, `redo`, `earlier`, and `later` for stream versions, on whatever keys you bound them to.
  Typed `:earlier`/`:later` in the stream find an empty history and do nothing.

**Save (phase 8).**

- `:w` in the stream writes every modified source the stream touched. It's one seam at the top of `write_impl`.
  It has to come before `write_impl`'s whitespace trims, which would otherwise edit removed rows and be reverted.
- `:wa` and the quit checks need nothing. The sources are ordinary documents, and the stream never counts as modified.
  `write_all_impl` skips documents that aren't modified.

**Until phase 9**, editing in the stream is plain text editing:

- no completion, hover, or goto
- new lines copy the previous line's indent, since the stream has no indent queries
- no format on type

### 3.10 LSP (phase 9)

Diagnostics come first, need no seam, and don't need editing. They could move into the read-only milestone.
A `DiagnosticsDidChange` hook gets `&mut Editor`, maps each source's diagnostics onto its rows, and calls `replace_diagnostics` on the stream.
Inline diagnostics, `]d`, and the diagnostics gutter then work as in any buffer. Only open files have diagnostics.

Goto, hover, references, rename, and code actions:

- They need stream versions that send the request for the source document, at the mapped position, bound in the stream's keymap.
- Upstream's versions read the current document. `commands/lsp.rs` has 19 `doc!`/`current!` call sites.
- The spike decides between copying their result handling and seaming them.

Completion is the likely first real core change:

- The request has to go to the source's servers, and the item's edits have to map back into the stream.
- The code is in `handlers/completion/` and `ui/completion.rs`.
- Preview inserts are ghost transactions: a savepoint restore, then the final edit. The sync forwards them all in order, so the source sees the preview too.

Rejected: switching the view to the source document while a command runs. Completion and goto answer asynchronously, after the switch back.

## 4. Upstream seams

One or two lines each, marked `// FORK:`. Commit counts are on upstream `master` for the year to 2026-10-17.

| File | Commits/yr | Seam | Phase |
| --- | --- | --- | --- |
| `helix-view/src/editor.rs` | 16 | Field `review` on `Editor`, and its default | 1 |
| `helix-view/src/editor.rs` | | One condition in `Editor::switch` that keeps the stream when you leave it | 1 |
| `helix-view/src/lib.rs` | 3 | `pub mod review;` | 1 |
| `helix-view/src/gutter.rs` | 3 | Stream branches in `line_numbers` and `diff` | 1 |
| `helix-term/src/ui/editor.rs` | 9 | Keymap field on `EditorView`, swapped in for the stream at the start of `handle_event` | 1 |
| `helix-term/src/ui/editor.rs` | | `review::sync` at the start of `render` | 1 |
| `helix-term/src/ui/editor.rs` | | `render_view`: one decoration, overlays, highlighter off for the stream | 4–5 |
| `helix-term/src/ui/editor.rs` | | Sync at the start of `handle_event` | 7 |
| `helix-term/src/keymap.rs` | | `pub mod review;` | 1 |
| `helix-term/src/handlers.rs` | 4 | `mod review;` and its `register_hooks` call, if a hook is needed | 7 |
| `helix-term/src/commands.rs` | 39 | Static command rows and one-line wrappers | 1, 3, 6, 8 |
| `helix-term/src/commands/typed.rs` | 18 | Typable rows and `mod` lines | 1–2, 6 |
| `helix-term/src/commands/typed.rs` | | The `write_impl` seam | 8 |
| `helix-vcs/src/git.rs`, `helix-vcs/src/lib.rs` | 3, 1 | `mod index;` lines, as in diff review phase 4 | 6 |
| `helix-view/Cargo.toml` | 10 | `imara-diff`, unless the diff goes through `helix-vcs` | 1 |
| `book/src/*` | | Commands, keys, theme scopes | each |
| `helix-term/tests/integration.rs` | 1 | `mod fork_review;` | 1 |
| `helix-term/src/commands/lsp.rs`, `handlers/completion/*`, `ui/completion.rs` | 6, 0, 1 | Not known until the phase 9 spike | 9 |

Not touched: `document.rs` (16/yr), `application.rs` (18/yr), `helix-core`.

## 5. Fork-owned files

- `helix-view/src/review.rs`: `Review`, the `Editor` field type. The row table, the stream's text, and lookups.
- `helix-view/src/review/layout.rs`: base + source + hunks + context → rows. Pure, unit-tested. Phase 1.
- `helix-view/src/review/reconcile.rs`: current rows vs wanted rows → one stream transaction. Pure, unit-tested. Phase 2.
- `helix-view/src/review/gutter.rs`: old and new numbers, signs. Phase 1.
- `helix-view/src/review/forward.rs`: checks stream changes and maps them to per-source transactions. Pure, unit-tested. Phase 7.
- `helix-view/src/review/sync.rs`: the read-only restore and refresh (phases 1–2), then the sync, loading sources, and the undo log (phases 7–8).
- `helix-term/src/handlers/review.rs`, from phase 7: `PostCommand`, `PostInsertChar`, `DiagnosticsDidChange`, and `DocumentDidClose` hooks, as editing needs them.
- `helix-term/src/commands/review.rs`: `:review`, `ret`, navigation, refresh, then undo and save.
- `helix-term/src/keymap/review.rs`: the stream keymap overlay.
- `helix-term/src/ui/text_decorations/review.rs`, `.../review/words.rs`: highlights, tints, header and gap rows, changed words.
- Phase 6: `helix-vcs/src/git/index.rs`, `helix-vcs/src/index.rs`, `helix-term/src/commands/typed/stage_hunk.rs`, restored from `archive/diff-review` and adapted.
- `helix-term/tests/test/fork_review.rs` and children: integration tests over a temporary git repository, as `fork_diff/review.rs` did.

## 6. Phases

| Phase | Status | Scope | Fork-owned lines (rough) | Risk |
| --- | --- | --- | --- | --- |
| 0. Plan | Done | Commit this plan and a `review-stream` ledger entry (6.1). | 0 | Low |
| **Read-only review** | | | **2.5–3.8k** | |
| 1. Read-only stream | Done | Layout, stream document, row table, gutter, `:review` against `HEAD`, the read-only guard, the keymap overlay, `ret` to the file. Headers and gaps as plain text. | 900–1,200. Actual: about 820, plus 580 of tests. | Medium: render seams, row math, the guard |
| 2. Keep it current | Done | Reconcile by identity, stale files re-diffed after each command, gaps that open, `:review-refresh`, `:review` switching to the open stream | 300–500. Actual: about 400, plus 310 of tests. | **High**: the cursor while rows appear and vanish |
| 3. Navigation | Done | Hunk and file motions across files | 150–300. Actual: about 180, plus 90 of tests. | Low |
| 4. Look | Done | Row tints, header and gap decorations, word highlights | 200–400. Actual: about 300, of it 130 ported, plus 100 of tests. | Low |
| 5. Syntax | Done | Highlights from full parses on both sides | 400–600. Actual: about 270, plus 80 of tests. | Medium: mapping highlight layers |
| 6. Staging | Planned | Index base, `unstaged`/`staged` reviews, stage and unstage, `:review-restore`, `:review-context`, picker jump | 500–800, about 600 of it ported | Low: ported code |
| **Editing, if it works out** | | | **1.1–1.7k** | |
| 7. Editing | Planned | Capture, sync points, checks, forwarding, loading sources, line endings, per-file indent | 800–1,200 | **High**: the engine |
| 8. Undo and save | Planned | Stream log, keymap swaps, `:w` | 300–500 | Medium |
| 9. LSP | Planned | Diagnostics, then a spike on goto, hover, and completion | 500–1,500 | **High**, unknown until the spike |

Phase 1 alone is a usable review: every hunk in one buffer, read-only editor features over it, and `enter` to edit.
Each later phase adds to it. Phase 2 comes second because it's the riskiest phase that the read-only review needs, and phase 7 reuses its reconcile.
If phase 7's relayout feels wrong in use, phase 7 is the place to stop, before undo and LSP build on it.

### 6.1 Phase 0: the plan

- There's nothing to revert. On 2026-10-17 `fork` was rebuilt from `eb31e577`, before the diff review plan's first commit.
  `FORK.md` has no `diff-review` entry, and `helix-term` has no `imara-diff` dependency.
- The diff review commits, `cb307282`..`dce1c2e8`, are on no branch. The local tag `archive/diff-review` keeps them from `git gc`.
  The tag is never pushed: `land.sh` pushes only `fork`.
  - `git show archive/diff-review:fork/diff-review.md` reads the old plan. Its research still holds: upstream PRs, and the diff lock hang in its 5.1.
  - `git checkout archive/diff-review -- <path>` restores a file (9).
- This plan goes in its own commit, before any code, as `AGENTS.md` asks. A `review-stream` entry in `FORK.md`, status planned, goes in the same commit.
  So does the fix to `fork/fork-infra.md`, which named `fork/diff-review.md` as a next step.

### 6.2 Phase 1: found

- The guard is one check at the start of `render`, not hooks.
  `r`, surround, and other on-next-key callbacks edit without a `PostCommand`, and a dispatched callback would draw the edit for a frame first.
  Phase 1 touches no `handlers.rs`.
- Clearing the stream's history has to keep the views in step. A view records the last revision it synced (`doc_revisions`).
  One left past the end of the new history panics in `History::changes_since`.
  `settle` syncs every view, swaps the history, then records revision 0 with an empty transaction.
- `Editor::switch` with `Action::Replace` deletes an unmodified buffer with no path when you leave it, and drops it from every jumplist.
  The stream is such a buffer, so `ret`, `:b`, or `ga` would destroy it, and `C-o` couldn't return.
  One condition in `switch` keeps it. Approved 2026-10-17 as the one seam inside an upstream function in a hot file.
- `GutterType::width` gets no `&Editor`, so `line_numbers_width` can't see the rows.
  The check sets `gutters.line_numbers.min_width` on the views that show the stream, every frame, and puts the configured width back on the others.
  `line_numbers_width` keeps no seam.
- The stream keymap is built when the stream gets focus: a clone of the user's keymap with `review_open` merged over it.
  A config reload while the stream has focus takes effect the next time it gets focus.
- Hunks drop the empty line after a final line ending, unlike the diff worker's.
  Otherwise a file that loses its final line ending gets a removed row for an empty line.
- A file's last line with no line ending gets `\n` in the stream. Phase 7's forwarding has to account for that one char.
- The git walk sees only the worktree. A file whose only change is unsaved in a buffer isn't listed. A listed file shows its buffer's text, unsaved edits included.
- `:review` diffs the repository of the current file, or the working directory for a buffer with no path, as diff review's did.
- Reading runs on the blocking pool. Decoding and diffing run on the main thread when the files arrive. Not measured on a large review.
- The integration tests are `fork_review.rs`, after `agents/TESTING.md`'s `fork_<area>` naming.
- `rustfmt` sorts `mod` lines, so the marker on `mod review;` in `commands.rs` is a trailing comment.

### 6.3 Phase 2: found

- No hook was needed. The check at the start of `render` compares each file's buffer, and its version, with the ones its rows came from.
  It follows them only while a view shows the stream, since `update` commits through a view of it.
- Keys by source line alone break on any insertion: every line below shifts, and every row would be replaced.
  So the old rows' lines go through a line map of the old text to the new, from a line diff.
  A changed line maps to the line that took its place, so an edited context row keeps its key, turns into an added row, and keeps the cursor.
- The layout was rewritten around runs of unchanged lines. A line is shown when it's within the context of a hunk, or in an opened range.
  Git's merging of nearby hunks falls out of that. Opened ranges are source lines, mapped through edits like the keys.
- Reconcile runs over the whole stream, with files matched by path. `:review-refresh` uses the same path, and a file that appears or vanishes inserts or deletes its rows.
- Helix maps a cursor on a replaced row to the start of what replaced it. A gap's cursor lands on its first shown line with no extra code.
- A kept row whose text changed has only the text before its line ending replaced, so the cursor stays on the row. It lands at the end of the new text. The column isn't kept.
- A new scratch buffer holds one line ending. The reconcile's last change runs to the end of the text to drop it.
- Run from the stream, `:review-refresh` listed the working directory's changes, since the stream has no path. The stream keeps the directory its changes were listed from.
- `update` puts back any pending edit to the stream first. Its transaction is built on the stream's text, and would panic on any other.
- A buffer that closes falls back to the file on disk, read on the main thread then. Not covered by a test.
- A file whose buffer opens with the same text costs one relayout of the whole stream, and no change to the buffer.
- With a review open, `:review` no longer lists the changes. `:review-refresh` does.

### 6.4 Phase 3: found

- `]g`/`[g` follow the rules of `]g` in a file: the next hunk starts below the cursor's row, the previous one ends above it. They select the hunk's removed and added rows, and extend in select mode.
- `]F` puts the cursor on the next header. `[F` from inside a file goes to that file's own header first, as `[[` does in vim.
- The stream keymap walks the user's keymap and swaps `goto_{next,prev,first,last}_change` for the review's versions, in sequences too. A custom key for them carries over, and the infobox shows the review's descriptions.
- A hunk is a run of removed and added rows. A header always separates two files' hunks, so hunks never span files.

### 6.5 Phase 4: found

- One seam in `render_view`, right after the syntax overlay: it adds one decoration and up to four overlays.
  The overlays go under diagnostics and the selection. The decoration comes after the cursorline, so a tinted row hides it, as in diff review.
- Row text is colored by overlays, not the decoration. A decoration runs before the text is drawn, so the text's own style would paint over its foreground.
- The scopes:
  - Tints are exact lookups of `diff.plus.view` and `diff.minus.view`. `diff.plus` and `diff.minus` are text colors in most themes.
  - Removed text uses `diff.minus.view`, falling back to `diff.minus`, so bundled themes show removed rows in their removal color.
  - Changed words use `diff.delta.view.text`, exactly, as diff review did. No bundled theme has it yet, so bundled themes show no changed words.
  - Headers use the new `ui.review.header`, falling back to `markup.heading`, and gaps the new `ui.review.gap`, falling back to `comment`.
  - The rule after a header uses `ui.window`, the split border.
- The rule is drawn before the text, from one column past the header's width, so the text never covers it. It's skipped on a header's wrapped lines.
- The style ranges and changed words are worked out once per update and kept on the stream, not per frame.
  `words.rs` came over from `archive/diff-review` without the clamp for a lagging diff, since rows never lag.
- Nothing here has an integration test. The tests can't see what's drawn. The ranges and the word diff have unit tests, and the rest is for the manual check.

### 6.6 Phase 5: found

- No seam turns the document highlighter off. The stream has no language, so it never had one.
- Syntax colors are overlays, one per nesting depth. An overlay holds one highlight per range, and a nested scope like a string inside a call needs both.
  The layers go in outermost first, so they patch in the order the highlighter's stack would.
- One highlighter runs per run of rows that show consecutive lines of one side of a file, over just those lines of the whole parse.
  Its char ranges map onto the stream by line, since a row holds its line's chars one to one.
- Each side of a file keeps its parse in the file, behind a lock the background parse fills. A finished parse requests a redraw.
  `update` hands a side's parse to the new file when that side's text is unchanged, so following an edit reparses only the side that moved, and `:review-refresh` reparses nothing that's the same.
- A source whose buffer is open and current lends the buffer's tree, which updates as you type, so it isn't parsed twice.
- Only the rows that can be in view get highlights. Nothing is parsed until a file's rows first show.
- As in diff review, removed rows get syntax colors only when `diff.minus.view` sets no text color. In most themes it falls back to `diff.minus`, a text color, and the base is never parsed.
- The language comes from the file's name, then a shebang in its source.
- The test parses Rust in place, through the bundled grammars, with no runtime.
  The buffer-tree path and the background parse have no test of their own. The integration tests can't see colors.

## 7. Risks

| Risk | Where | Mitigation |
| --- | --- | --- |
| Cursor jumps when rows appear or vanish | Reconcile, phase 2 | Match rows by identity. Unit tests on layout transitions: context → modified, modified → context, hunks merging and splitting. |
| An edit slips past the read-only guard | Guard, phase 1 | The check runs before every frame and compares the buffer's version, so any edit is put back before it's drawn. |
| Stream and source drift apart | Sync, phase 7 | One queue in event order. The fork's own edits are flagged. Debug builds assert after each sync that every editable row equals its source line. |
| Undo surprises | Stream log, phase 8 | Undo only when the source's revision matches. Otherwise stop and name the file. |
| An edit refused halfway through a command | Checks, phase 7 | Revert the whole window, explain in the status line. `c` over a removed row reverts the delete but still leaves you in insert mode. |
| Slow big reviews | Refresh, reconcile | Re-diff only stale files. Size caps per file. Load sources only when edited. |
| Upstream churn | `ui/editor.rs` (9/yr), `commands.rs` (39/yr) | Seams stay one line. Bodies live in fork files. |
| LSP needs core changes | Phase 9 | Spike first. Diagnostics need none. Without the rest, the stream still works for editing. |

## 8. Open decisions

1. **Relayout timing**, for editing. Live as you type, like Zed (recommended), or deferred until `Esc`, with rows fixed while in insert mode.
2. **`staged` scope**, for phase 6. Read-only even with editing, since the index isn't a file. The alternative is to drop the `staged` scope.
3. **Undo when a source moved on**, for phase 8. Stop and name the file (recommended), or undo anyway in that file's own history.
4. **LSP scope**, for phase 9. Which of diagnostics, goto, hover, and completion are must-haves. Completion decides whether phase 9 touches core code.

Decided, 2026-10-17:

- **Order:** the read-only review first, editing last, so there's a basic review to build on.
  `AGENTS.md` puts the riskiest phase as early as its dependencies allow, which would put editing right after phase 2.
  The cost: phases 4–5 may need rework when relayout goes live in phase 7.
- **No default key for `:review`.** You bind it, for example `[keys.normal.space] v = ":review"`.
  A row in `keymap/default.rs` would conflict if upstream took the key.
- **The stream's keys are fixed at first.** They replace commands, not keys, so your bindings for `goto_next_change` and the like still apply.
  A `[keys.review]` table can come later.
- **The stream is named `[scratch]`.** `Document::display_name` has no hook, and `[review]` would cost a seam in `document.rs`.
- **`]g`/`[g` select the whole hunk**, removed and added rows, and **`]F`/`[F` put the cursor on a header**.
- **Phases 3–6 run back to back**, one commit each, and the human checks the read-only review at the end.
- **`Enter` on a gap shows all of its lines**, and opened gaps stay open through edits and `:review-refresh`. Before phase 2, it opened the file.
- **No hunk header rows** (`@@ -a,b +c,d @@`) at first. The gutter's old and new numbers already place each hunk. A toggle, like Hunk's `m`, can come later.

## 9. Reuse from diff review

From the `archive/diff-review` tag, restored with `git checkout archive/diff-review -- <path>` when a phase needs them:

- `helix-term/src/ui/text_decorations/diff/words.rs`: word diff, phase 4.
- The base parse cache in `helix-term/src/ui/text_decorations/diff.rs` (`InlineDiffState`), phase 5.
- `helix-vcs/src/git/index.rs`, `helix-vcs/src/index.rs`, `helix-term/src/commands/typed/stage_hunk.rs`, phase 6.
- File kinds in `helix-term/src/commands/review.rs` (new, deleted, renamed, conflict), phase 1.
- The test harness helpers in `helix-term/tests/test/fork_diff/review.rs`: a temporary repository, the idle-timer re-arm.

Findings that still hold, from the old plan (`git show archive/diff-review:fork/diff-review.md`). Section numbers are that plan's:

- Copy out of the diff lock, never hold a `Diff` across a frame (5.1). The stream computes its own hunks, so this only matters if it reads the gutter's diff.
- The idle timer in the integration test harness needs re-arming (5.2).
- `git` runs with the `--git-dir` and `--work-tree` gix found, and only in a trusted workspace (5.3).

## Sources

- Hunk: [keyboard and mouse](https://www.hunk.dev/docs/start/keyboard-and-mouse/), [layout and display](https://www.hunk.dev/docs/configure/layout-and-display/), [modem-dev/hunk](https://github.com/modem-dev/hunk)
- Zed: [Split Diffs are Here](https://zed.dev/blog/split-diffs), [Zed 1.11.3](https://zed.dev/releases/stable/1.11.3), `crates/multi_buffer/src/transaction.rs` in [zed-industries/zed](https://github.com/zed-industries/zed)
- [helix#15611](https://github.com/helix-editor/helix/issues/15611) and matoous's [`md/multibuffer`](https://github.com/matoous/helix/tree/md/multibuffer) branch
- [helix#15631](https://github.com/helix-editor/helix/pull/15631) for the `.view` theme scopes
