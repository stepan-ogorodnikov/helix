# Helix fork: a review stream

Snapshot: fork branch `fork` at `81f68636`, on upstream `master` at `ba40e547`.
Reference views: Hunk's unified layout ([hunk.dev](https://www.hunk.dev/)), Zed's project diff.
Supersedes the diff review plan. Its code never reached the rebuilt `fork`.
Its plan and code stay under the local tag `archive/diff-review` (6.1).

Status: phase 9e done 2026-10-20 (6.25). Phases 1–9e are complete, and the manual check passed 2026-10-19 after three fixes (6.23). Signature help, 9f, is next.

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
- **Size:** about 2.5–4k fork-owned lines for the read-only review, plus tests. Editing adds 0.9–1.5k. LSP adds 0.4–0.65k for diagnostics, goto, hover, code actions, and rename. Completion and signature help would add about 0.35–0.45k, by the 9d spike.
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
  It runs on upstream's public API (`Document::apply`, `history`, `reset_modified`, `indent_style`, `line_ending`, `replace_diagnostics`), and for diagnostics on the `DiagnosticsDidChange` event.

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
| `u` `U` `A-u` `A-U` | the stream log (3.9) | the document history | 8a |

`ret` records a jump first, so `C-o` returns to the stream. `commands/review.rs` is a child of `commands`, so it can call the private `push_jump`.

Typable: `:review [all|unstaged|staged]`, `:review-refresh`, `:review-context <n>`, `:stage-hunk`, `:unstage-hunk`, `:review-restore`.
`:review-restore` writes the base text over the hunks under the selection into the source, like `:reset-diff-change`, and the refresh removes them from the stream.
It opens the file's document if it isn't open, so the restore can be undone there, and from 8a by `u` in the stream.

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

**Loading.** The stream opens a file's document with `Action::Load` when an edit reaches its section, as `:review-restore` does.
`Action::Load` gives the document a selection for the current view and no view of its own.
A file that loads with text different from what the stream showed gets its section rebuilt, and the edit is refused.
Closing the stream closes the documents it loaded that are unmodified and that no view showed while it was open (7d).

**Capture.** The stream's own history holds exactly its edits since the last sync, because each sync clears it (`settle`).
The sync commits the pending ones and reads `changes_since(0)`: one exact changeset, undo and redo inside the window included.
A text diff of the stream's text wouldn't do. It can't tell a deleted context row from an equal removed row next to it.
So there's no `DocumentDidChange` hook and no queue.

**Sync points**, each with `&mut Editor`: the start of `EditorView::render`, as in phase 1, and the start of `EditorView::handle_event`. Seams.
Every key goes through `handle_event`, a macro's too, since `replay_macro` feeds its keys to the compositor. So the next key always sees current rows.
The `render` sync catches what comes without a key: a typable command's edit, LSP edits, job callbacks.
No `PostCommand` or `PostInsertChar` hook is needed.

**A sync:**

1. **Check the stream's edits.** Each change must fall inside one run.
   - A run's text must still end with a line ending, or be empty. So the line break that ends a run can't be deleted, and text inserted at a run's end has to end in one. The stream can't show the join.
   - An insert at a run's start or end belongs to the run.
   - `O` on a run's first row inserts a line ending and indent at the end of the row above it, which isn't editable.
     An insert there that starts with that row's line ending is read as the same line inserted at the run's start.
   - A file's last line with no line ending shows one in the stream (6.2). It stands for the end of the file. An edit that reaches that line makes it real, unless the edit deletes it (6.13).

   If any change breaks a rule, the whole window reverts, as in phase 1, and a status message says why.
2. **Forward.** Shift each change by its run's offset into the file, and apply one transaction per file with the stream's view id.
   The file's selection for that view goes to the edit first, so `u` in the file puts the cursor there.
   A file whose buffer moved since its rows were built refuses the edit, and its rows follow the buffer. That takes an LSP edit and a key with no sync between them.
3. **Map rows.** The row table moves through the edit: each run becomes the file's new lines in its range, keyed by their new line numbers.
4. **Reconcile** each touched file, as in phase 2, from the stream's text after the edit.
5. **Tidy.**
   - Clear the stream's history, and `reset_modified`, so the stream never counts as modified.
   - Outside insert mode, commit each touched file's changes to its history. One stream command, or one insert session, is one undo step in the file.
   - Set the stream's `indent_style` and `line_ending` to those of the file under the primary cursor.

**No echo.** Reconcile compares the rows a file should have with the rows it has.
A forwarded edit already made them equal, so it causes no second edit.

**Relayout is live, as in Zed** (decided 2026-10-18).
Edit a context line, and its old version appears above it as a removed row. The line itself turns into an added row.
The new row is an insert before the cursor's line, so the cursor stays on the text it was on.
Change an added line back to the base text, and its removed twin vanishes.

**Completion.** Word and path completion work in any buffer, the stream included.
A completion's preview restores a savepoint first, which reverts every edit since, the stream's relayouts too.
A reverted removed row would come back as text inserted into a run, and be forwarded into the file.
So while a completion menu is open in the stream, the sync forwards edits and maps rows, but holds the reconcile until the menu closes.
The seams pass whether a menu is open, from `EditorView::completion`.

**Undo and redo (phase 8a).**

- The stream's own history is never used. Its revisions would mix your edits with relayouts.
- The stream keeps a log of steps. A step is one stream command, and lists each file buffer it changed, with its revisions before and after.
  A stream command is one normal-mode command, or one insert session ending at `Esc`. Phase 7 already commits each as one revision of each file it touched.
  A `:review-restore` is a step too, and so are a `:w`'s own edits (8b).
- `u` in the stream pops a step and undoes it in each of its files, through the stream's view.
  If any file moved on since, for example you edited it in another split, nothing is undone, and the status line names the file. `ret` then `u` undoes it there. A closed file stops it the same way.
- `U` is the same in reverse. A file whose redo would take a newer branch of its history, one made in its own split after the undo, stops it too.
- A count undoes that many steps, and stops at the first that can't go. A new step drops the steps there were to redo.
- Afterwards the rows follow the files at once. The stream gets a cursor at each file's cursor, which the file's history put back where the step began.
  The cursor goes on that line's row, or on the file's header when the line isn't shown. The primary is the first file's in the stream.
- The stream's keymap swaps `undo` and `earlier` for `review_undo`, and `redo` and `later` for `review_redo`, on whatever keys you bound them to.
  The log is a line, so one step earlier is one undo. Typed `:earlier`/`:later` in the stream find an empty history and do nothing.
- `review_undo` from insert mode, on a key you bound there, ends the session's step first, as upstream's `undo` commits first.
- A staged review refuses both, as it refuses edits: its rows show the index, not the files.
- The log lives on the stream, so it stays through `:review-refresh` and a change of scope, and closes with the stream.
  Each file's own history still has its revisions then.

**Save (phase 8b).**

- `:w` in the stream writes every file the stream shows that has unsaved changes, however they were made (decided 2026-10-18).
- It's one seam at the top of `write_impl`.
  It has to come before `write_impl`'s whitespace trims, which would otherwise edit removed rows and be put back.
- The body is `commands/typed/review_write.rs`, a child of `typed`, so it can call the private trims and `make_format_callback`.
  It runs `write_all_impl`'s chain over those files: trims, final newline, code actions and format on save, then the save.
  It's a copy, about 60 lines, since `write_all_impl` writes every modified buffer and takes no filter. Its `WHY` names `write_all_impl`, and the ledger lists it, so a rebase that changes the chain updates the copy.
- `:w!` forces, and `--no-format` and `--no-code-actions` pass through. `:w <path>` in the stream fails, since each file has its own path.
  With nothing to write, the status line says so.
- The save's own edits are steps in the log (decided 2026-10-18), so `u` after `:w` undoes them, as `u` in a file undoes a format first.
  - The trims and final line endings of all the files are one step.
  - A format lands later, in upstream's callback. The copy wraps the callback, and logs each file's format as a step of its own when it lands.
  - Code actions on save aren't logged. A file they change stops `u` in the stream, as any edit made elsewhere does.
- `:wq` and `:wbc` go through `write_impl`, so they write the files too.
- `:x` and `:update` write only a modified buffer, and the stream never counts as modified, so in the stream they write nothing (decided 2026-10-18).
  `:x` closes the view. Quitting the last view is still refused while the files have unsaved changes. The book says to use `:w`.
- `:wa` and the quit checks need nothing. The sources are ordinary documents, and the stream never counts as modified.
  `write_all_impl` skips documents that aren't modified.

**Until phase 8a**, `u` in the stream finds an empty history, and `ret` then `u` undoes in the file.
**Until phase 8b**, `:w` in the stream fails, as in phase 1, and `:wa` writes every file.

**Until phase 9a**, the stream has no LSP hover or goto. LSP completion waits for 9e, and signature help for 9f. Until then, editing in the stream is plain text editing:

- no LSP completion. Word and path completion work, as in any buffer.
- new lines copy the previous line's indent, since the stream has no indent queries
- no format on type

### 3.10 LSP (phase 9)

Language servers know only files that are open in a buffer, and the stream isn't one of them: it has no path and no servers.
So each LSP feature in the stream works through the file under the cursor. Scope decided 2026-10-18 (8).

**Goto, references, hover, and code actions (9a): the stream lends its view.**

- Upstream's commands build their request from the focused view's buffer at once, and their answers never read that buffer again (6.19):
  - goto and references jump from the current view, so `C-o` returns to the stream;
  - hover's popup sits at the current view's cursor;
  - a code action applies its edits by path, through its server.
- So the stream's version points the focused view at the file's buffer, with the selection at the cursor's place in the file, calls upstream's command, and points the view back.
  All of it runs inside one key, with no render between.
- The file's buffer is loaded first, in the background, as an edit loads it, and closes with the stream (7d). Nothing is preloaded.
  When the file has no running server yet, the status says its server is starting, instead of upstream's `No definition found.` The key works again once the server is up.
- A cursor on a removed row, a header, or a gap has no place in the file, and the command says so. A staged review refuses, since its rows show the index.
- The stream's keymap swaps `goto_definition`, `goto_declaration`, `goto_type_definition`, `goto_implementation`, `goto_reference`, `hover`, and `code_action` for the lending versions, on whatever keys you bound them to.
  The versions are built in `keymap/review.rs` as `MappableCommand::Static` and aren't in the command table. They exist only as swaps, so `commands.rs` grows no rows.
- Not lent, and left as upstream has them:
  - workspace symbols and select-references read the focused buffer again in their answers;
  - document symbols and the diagnostics picker show one file's, and the stream has no single file to show.

**Diagnostics (9b).**

- Each file's diagnostics go onto its rows.
  An open file's come from its buffer, which moves them as you type.
  A file that isn't open gets what its servers reported for it, which Helix keeps by path. Many servers report only open files. rust-analyzer's `cargo check` reports the whole workspace.
- A diagnostic goes on the row of its source line, at the same column. One that spans lines ends on the row of its last line, or at its first row's end when that line isn't shown.
  Removed rows, headers, and gaps get none.
- No hook. The check at the start of `render` and `handle_event` compares each file's report with the one it last placed, as it compares versions (6.3).
  A report for a file that isn't open dispatches no event anyway. A relayout places them all again, which covers an open buffer's diagnostics moving as you type.
- In the stream they then work as in any buffer: inline diagnostics, `]d`, `[d`.
- The diagnostics gutter shows only diagnostics from the buffer's own servers, and the stream has none.
  A stream branch at the top of `gutter.rs`'s `diagnostic` draws the dot without that filter, as `line_numbers` and `diff` have their branches.

**Rename (9c).**

- Rename's prompt reads the focused buffer again on confirm, when it's the stream again, so it can't be lent.
- The stream's own rename opens a prompt prefilled with the word under the cursor. On confirm it sends the request to the file's server, at the place the cursor had, and applies the edit as upstream does.
- It leaves out `prepareRename`, the server's own range and placeholder. A file that changed while the prompt was open refuses the rename.

**Completion and signature help (9e–9f, from the 9d spike, 6.24).**

- They start from the completion handler while you type, not from a command, so the view can't be lent for all of it.
- The requests: a seam in `request_completions` adds the servers of the file under the cursor to the stream's own word and path completion.
  `review::lend` readies the file's buffer, and upstream's `request_completions_from_language_server` asks from it. The savepoint stays the stream's.
- The items keep the file's places. A row holds its file's line char for char, so an edit moves onto the stream by changing its line numbers to those of the rows that show them, in any offset encoding.
  The menu's preview and accept arms (`ui/completion.rs`) each move the item right before they turn it into a transaction, after any resolve. The menu applies it to the stream like any item, and the sync forwards it.
  An edit whose lines aren't on rows one after another, as one over a hidden line or across a removed row, goes in at the cursor instead.
- An item's other edits, like an import, are places in the file, usually on lines the review doesn't show.
  The accept arm takes them, and the next sync makes them in the file's buffer right after it forwards the item.
- Preview inserts are ghost transactions: a savepoint restore, then the final edit. The sync forwards them all in order, so the source sees the preview too.
- Typing into an open menu asks again for an incomplete list, through a seam at the top of `request_incomplete_completion_list`.
  That runs inside the key, before the frame's sync has forwarded the char, so it runs the sync first, held as under a menu.
- Signature help lends the view inside `request_signature_help`. Its answer reads only the focused buffer's language.
- Trigger characters come from the file's servers, through one seam in `trigger_auto_completion` and one in signature help's insert hook.

Rejected:

- Lending the view for completion. Its answer arrives after the view shows the stream again.
  Goto and hover answer late too, but their answers don't read the buffer they asked from.
- Rewriting an item's line numbers when the answer arrives. A resolve can answer with the file's places again (6.24).

## 4. Upstream seams

One or two lines each, marked `// FORK:`. Commit counts are on upstream `master` for the year to 2026-10-17.

| File | Commits/yr | Seam | Phase |
| --- | --- | --- | --- |
| `helix-view/src/editor.rs` | 16 | Field `review` on `Editor`, and its default | 1 |
| `helix-view/src/editor.rs` | | One condition in `Editor::switch` that keeps the stream when you leave it | 1 |
| `helix-view/src/lib.rs` | 3 | `pub mod review;` | 1 |
| `helix-view/src/gutter.rs` | 3 | Stream branches in `line_numbers` and `diff` | 1 |
| `helix-view/src/gutter.rs` | | Stream branch in `diagnostic` | 9b |
| `helix-term/src/ui/editor.rs` | 9 | Keymap field on `EditorView`, swapped in for the stream at the start of `handle_event` | 1 |
| `helix-term/src/ui/editor.rs` | | `review::sync` at the start of `render` | 1 |
| `helix-term/src/ui/editor.rs` | | `render_view`: one decoration, overlays, highlighter off for the stream | 4–5 |
| `helix-term/src/ui/editor.rs` | | Sync at the start of `handle_event`, and whether a completion menu is open, passed to both syncs | 7a |
| `helix-term/src/keymap.rs` | | `pub mod review;` | 1 |
| `helix-term/src/commands.rs` | 39 | Static command rows and one-line wrappers | 1, 3, 6, 8a |
| `helix-term/src/commands/typed.rs` | 18 | Typable rows and `mod` lines | 1–2, 6, 8b |
| `helix-term/src/commands/typed.rs` | | The `write_impl` seam | 8b |
| `helix-vcs/src/git.rs`, `helix-vcs/src/lib.rs` | 3, 1 | `mod index;` lines, as in diff review phase 4 | 6 |
| `helix-view/Cargo.toml` | 10 | `imara-diff`, unless the diff goes through `helix-vcs` | 1 |
| `book/src/*` | | Commands, keys, theme scopes | each |
| `helix-term/tests/integration.rs` | 1 | `mod fork_review;` | 1 |
| `helix-term/src/handlers/completion/request.rs` | 0 | `pub(super) mod review;`, the file's requests in `request_completions`, and the review's re-request at the top of `request_incomplete_completion_list` | 9e |
| `helix-term/src/handlers/completion.rs` | 0 | The file's trigger characters in `trigger_auto_completion` | 9e |
| `helix-term/src/ui/completion.rs` | 1 | The item moved onto the rows in the menu's preview arm and accept arm | 9e |
| `helix-term/src/handlers/signature_help.rs` | 1 | `mod review;`, the lend at the top of `request_signature_help`, and the file's trigger characters in the insert hook | 9f |

Not touched: `document.rs` (16/yr), `application.rs` (18/yr), `helix-core`.

## 5. Fork-owned files

- `helix-view/src/review.rs`: `Review`, the `Editor` field type. The row table, the stream's text, and lookups.
- `helix-view/src/review/layout.rs`: base + source + hunks + context → rows. Pure, unit-tested. Phase 1.
- `helix-view/src/review/reconcile.rs`: current rows vs wanted rows → one stream transaction. Pure, unit-tested. Phase 2.
- `helix-view/src/review/gutter.rs`: old and new numbers, signs. Phase 1.
- `helix-view/src/review/forward.rs`: checks stream changes and maps them to per-source transactions. Pure, unit-tested. Phases 7a–7b.
- `helix-view/src/review/sync.rs`: the read-only restore and refresh (phases 1–2), then the sync and loading sources (phase 7).
- `helix-view/src/review/log.rs`: the undo log, and undo and redo across the files. Phase 8a.
- `helix-view/src/review/diagnostics.rs`: places each file's diagnostics on its rows. Phase 9b.
- `helix-term/src/commands/review.rs`: `:review`, `ret`, navigation, refresh, then undo and redo.
- `helix-term/src/commands/typed/review_write.rs`: `:w` in the stream. Phase 8b.
- `helix-term/src/commands/review/lsp.rs`: the lending versions of goto, references, hover, and code actions, and the stream's rename. Phases 9a and 9c.
- `helix-view/src/review/complete.rs`: moves a completion item onto the rows, and keeps its other edits for the file. Phase 9e.
- `helix-term/src/handlers/completion/request/review.rs`: the review's completion requests and trigger characters, a child of `request` so it can call upstream's private request builder. Phase 9e.
- `helix-term/src/handlers/signature_help/review.rs`: signature help with the view lent, and its trigger characters. Phase 9f.
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
| 6a. Scopes | Done | Index base, `:review unstaged` and `:review staged` | Actual: about 120, plus 300 ported, plus 100 of tests. | Low: ported code |
| 6b. Staging | Done | `:stage-hunk` and `:unstage-hunk` on the hunks under the selection | 200–300, most of it ported. Actual: about 230, of it 130 ported, plus 150 of tests. | Low: ported code |
| 6c. Restore and context | Done | `:review-restore`, `:review-context` | 150–250. Actual: about 130, plus 110 of tests. | Low |
| 6d. Picker key | Done | A changed-files picker key that jumps into the review at the file's header | 50–100. Actual: about 80, plus 20 of tests. | Low |
| **Editing, if it works out** | | | **0.9–1.5k** | |
| 7a. In-line edits | Done | An edit inside a line of an added or context row reaches the file's buffer, loaded if need be, and the rows relayout live. Capture, the `handle_event` sync, the check, forwarding, commits, the hold under a completion menu. Edits that add or remove a line are refused. | 300–450. Actual: about 360, plus 340 of tests. | **High**: the engine |
| 7b. Line edits | Done | Edits that add or remove lines: the rules at a run's edges, `O` at a run's start, a last line with no line ending, mapping rows, the file's line ending | 250–400. Actual: about 240, plus 180 of tests. | Medium: the rules at run edges |
| 7c. Indent | Done | The stream takes the indent style of the file under the cursor | 20–50. Actual: about 45, plus 30 of tests. | Low |
| 7d. Cleanup | Done | Closing the review closes the files it loaded that are unmodified and that no view showed | 30–60. Actual: about 35, plus 45 of tests. | Low |
| 8a. Undo | Done | `u` and `U` in the review undo and redo its edits and restores in their files, by the review's own log | 200–300. Actual: about 290, plus 180 of tests. | Medium: histories across files, the cursor after |
| 8b. Save | Done | `:w` in the review writes the files it shows with unsaved changes, and logs the save's own edits | 120–200. Actual: about 185, of it 60 copied, plus 125 of tests. | Low: a copied chain, and one wrapped callback |
| **LSP** | | | **0.4–0.65k** | |
| 9a. Goto and hover | Done | Goto, references, hover, and code actions from the stream, by lending its view to the file | 150–250. Actual: about 150, plus 120 of tests. | Medium: the lend has to leave no trace |
| 9b. Diagnostics | Done | Each file's diagnostics on its rows, open or not, with the gutter's dot | 150–250. Actual: about 190, plus 105 of tests. | Low |
| 9c. Rename | Done | Rename from the stream, through its own prompt | 80–150. Actual: about 95, plus 10 of tests. | Low |
| 9d. Completion spike | Done | A prototype of completion and signature help from the file's servers, tested against rust-analyzer (6.24) | Actual: about 400, plus 240 of tests, on the local branch `spike/review-completion` | **High**: seams inside upstream functions |
| 9e. Completion | Done | Completion from the servers of the file under the cursor: requests, re-requests, trigger characters, items moved onto the rows, and their other edits made in the file | 250–350. Actual: about 350, plus 28 in seams and 420 of tests. | Medium: the order of an item and its other edits |
| 9f. Signature help | Planned | Signature help from the file's server, by lending the view, with its trigger characters | 60–100 | Low |

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
  A selection on the gap row maps over every line it shows, though, so opening a gap now leaves a cursor on the first one (8, 2026-10-19).
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

### 6.7 Phase 6a: found

- Phase 6 split into 6a–6d, one line each, as `AGENTS.md` asks of a phase.
- `git/index.rs` and its facade came back from `archive/diff-review` with only their module comments and tags changed. Reading the index needs no `git` binary.
- The scope lives on the stream. `:review` with no scope switches to the open review. A different scope reads the changes again, keeping the place of every row that still stands for the same line.
- A staged review's sources are the index, so it never takes a file's buffer, never follows one, and lends no buffer's tree for syntax.
  `Enter` still opens the file, at the index line's number.
- An `unstaged` review diffs a conflicted file against `HEAD`, as diff review ruled. The index has no unconflicted version.
- Each read sets a status that names the scope, since nothing else on screen shows it.
- Helix prefixes a typable command's error with its name: `'review': 'head' is not one of all, unstaged, or staged`.

### 6.8 Phase 6b: found

- The review picks hunks by its own rows: every review hunk the selection touches. Each becomes its source lines, first and last, and a removal the line below it.
  From there diff review's code runs as it did on a buffer's lines: it diffs the index against the source and stages the hunks under those lines.
- The source is the review's own: a buffer's text, unsaved edits included, as in diff review, or the index in a staged review, where `:stage-hunk` finds nothing to stage.
- `:unstage-hunk` maps the lines to `HEAD`'s through the `HEAD`-to-source diff, so it works the same in all three scopes.
- Both read the review again afterwards, so a staged hunk leaves an `unstaged` review and joins a `staged` one. The status after the read says what they did, unless the review is now empty.
- Rows that refill an empty stream pushed the cursor past them, and `]g` then found nothing. A stream that was empty starts at the top again, like a new one.
- Staging writes in the source's encoding, which `Section` now carries, and keeps the index version's byte order mark.
- `run_until` in the tests names the status when it gives up. A wait for the index alone can pass before the review's read lands.

### 6.9 Phase 6c: found

- `:review-restore` picks hunks as staging does, with the same `under`, and diffs the base against the file's buffer, not the review's source, so it never restores text the buffer has moved past.
  A file that isn't open gets a buffer in the background. Nothing is saved, and `u` in the file undoes the restore.
- A staged review refuses it: its new side is the index, and `:unstage-hunk` already takes a hunk back out.
- The context is a field of the stream. `:review-context` lays the files out again through `update`, so the cursor and opened gaps stay.

### 6.10 Phase 6d: found

- The key is `review_jump` on `Alt-r`, unused in pickers and in the prompt, as diff review had it. Other pickers pass it to the prompt, like `cycle_changes`.
- The picker hands the jump its current list, so the unstaged list jumps into an `unstaged` review. A review of that scope that has the file only moves the cursor. Anything else reads the list's changes first, then puts the cursor on the header.
- The hook sits next to `changed-files-views`' `on_cycle_changes` in `ui/picker.rs`: a type alias, a field, its default, a builder, and a match arm.
  `picker_keys.rs` and `changed_file.rs` belong to other fork features. This feature adds a variant to the first and a hook call to the second.
- The picker walks the working directory, which the tests can't change, so the key has no end-to-end test. `focus_file` has one.

### 6.11 Phase 7: the split

Planned 2026-10-18, after the human tried the read-only review. Nothing here is built yet.

- Phase 7 split into 7a–7d, one line each:
  - 7a: Forward edits within a line of the review to the file's buffer.
  - 7b: Forward edits that add or remove lines in the review.
  - 7c: Indent the review like the file under the cursor.
  - 7d: Close the files the review loaded when it closes.
- 7a carries the engine, so it goes first. Without a line added or removed, the row table keeps its shape, and phase 2's line maps move it as they do now.
  7b adds the rules at a run's edges and the mapping by new line numbers. A typed new line has to keep its key, or reconcile replaces it and the cursor loses its column (6.3).
- The line ending goes with 7b, the first phase that inserts one. 7c is the indent only.
- The edits are read from the stream's own history, not captured by a hook (3.9). The old plan's `DocumentDidChange` queue and its replay went with it, and so did the `handlers.rs` seam until phase 9.
- A race between the stream and a file's buffer refuses the stream's edit, instead of replaying both in order. It needs an edit with no key or frame between it and the next.
- Word and path completion are on in any buffer, the stream included. That's why the reconcile is held while a menu is open (3.9).
- `open` inserts `O`'s line at the end of the line above (`commands.rs`, `fn open`). That's why the rule at a run's start exists.
- Each relayout diffs every file and finds the changed words of every hunk again. 7a measures a keystroke in a large review, and keeps each untouched file's hunks if it's slow.
- `:review-restore` commits with the stream view's selection in the file. `Action::Load` puts that at the file's start, so `u` in the file likely moves the cursor there.
  7a checks it, and fixes it with the helper that sets the selection at the edit.

### 6.12 Phase 7a: found

- The capture works as planned: commit the stream's pending changes, then read `changes_since(0)`. An edit undone before the sync leaves no changeset, so the sync only settles.
- `put_back` left insert mode whenever the stream was in it. Phase 1 wanted that, since entering insert mode was already refused.
  Every relayout calls it, so one typed char dropped you into normal mode, and the next key ran as a command. It now leaves insert mode only to put an edit back.
- A refusal still leaves insert mode, so the rest of what you type runs as normal-mode keys, as in phase 1. `ihello` on a removed row moves the cursor with `e`, `l`, `l`, and opens a line with `o`.
- Upstream clears the status on every key. A refusal found by the sync at the start of `handle_event` is cleared by that same key.
  In use, each key is followed by a frame, whose sync finds the edit first, so the message shows. A refusal inside a macro shows none.
- A forwarded edit is committed to the file's history outside insert mode, so an insert session is one undo step. The test types two chars and undoes both with one `u`.
- `:review-restore`'s cursor bug was real: `u` in the restored file put the cursor at the top. `apply_to_file` sets the selection at the first change, for restore and forwarding both.
- An open file is checked by version, since its rows came from that version. A file the edit loads is checked by text.
- While a completion menu holds the reconcile, the rows' kinds lag: an edited unchanged line shows no sign until the menu closes. The ranges move through the edit, so tints stay on their rows.
- Accepting a completion forwards a replace of the preview by the same text. The file gets a change that changes nothing.
- The completion test uses path completion. Upstream's test config turns word completion off, since its hook leaks between tests.
  Without the hold, accepting the item is refused and you leave insert mode, which is what the test checks.
- Cost of one relayout, release build, measured on made-up files:
  - Before: laying out 200 files of 2,000 lines (9,400 rows) took 124 ms, nearly all of it diffing every file again. 50 files of 500 lines took 8 ms.
  - Each file now keeps its hunks while neither side changes. A key in the 200-file review costs 27 ms in all, 5 ms of it the layout. 500 files cost 59 ms.
  - The rest is whole-stream work: reconcile, the ranges and their changed words, and the line maps. Limiting it to the edited file's rows is the next step, if that's slow in use.
- `update` kept a passed-in file's parse when that side's text changed. `follow_sources` passes clones, so a buffer that closed was highlighted with its old parse until something else changed. A changed side now starts over, for parses and hunks both.
- Only a staged review sets `readonly`, so only it shows as read-only in the statusline.

### 6.13 Phase 7b: found

- The join rule is checked on the new text. Each run's span moves through the edit, with an insert at either edge its own, and has to come out empty or ending in a line ending.
- `O` below a header in a CRLF file inserted `\r\n` where the header ended in `\n`, so the rule at a run's start didn't apply and the edit was refused.
  Headers, gaps, and a last line's shown line ending now take the file's line ending: the first one in its first 100 lines, or LF.
- The stream's line ending follows the file under the cursor on every sync, so the statusline shows that file's.
- The plan's rule for a last line with no line ending was too narrow. `o` on that line leaves an empty last line, which a file can only have with a line ending after it.
  The row would vanish on the relayout and take the cursor along. Now any edit that reaches the line makes its shown line ending real, unless the edit deletes it. Helix adds a final line ending on write by default anyway.
- Rows move through the edit by line count. A run whose count changed comes up as added rows until the relayout, keyed by the file's new lines. The gaps below it move by the same count.
- Opened gaps map through a diff of the file's old and new text. They don't key rows, so a diff's guess is fine there.
- The debug check now also wants one row per line, and no row for a line the file doesn't have. It caught the empty last line above.

### 6.14 Phase 7c: found

- An open buffer lends its own indent, which has its editorconfig and language in it.
  A file that isn't open gets the indent its text shows, then its language's unit, then the stream's own, which is upstream's default for a new buffer. That default is private to `document.rs`, so the stream keeps the one it started with.
- Detection reads up to 1,000 lines, too much for every frame. Each file keeps what it found until its text changes, as with its hunks.
- A file that isn't open goes without its editorconfig until an edit opens it, so only the first key in it can miss.
- The line ending stays the one the text shows, even with the buffer open, so it agrees with the headers' (6.13).

### 6.15 Phase 7d: found

- The sync already notices when the stream's buffer is gone, so closing the files there needs no `DocumentDidClose` hook. They close at the next sync.
- `:review-restore` opens files in the background too. It now goes through the same `review::load`, so its files close with the review as well.
- A file shown in a view since, by `ret` for example, stays open, and so does one with unsaved changes, as any buffer would.
- The manual check (2026-10-19) found a saved file left open when it was the only other buffer. Helix gives the closed review's view to another buffer before the next sync, so a view showing the file then said nothing about you.
  A loaded file a view shows at a sync is now yours and leaves the list, and at the close every file still on it closes. A file opened with `ret` and left with `C-o` used to close with the review, since `C-o` takes it out of the view's history. Now it stays.
- A one-letter search in the tests could match the random temp directory in a header row, about one run in ten for `/B`. The test anchors it, and an older `/x5` too.

### 6.16 Phase 8: the split

Planned 2026-10-18, after the human tried the editing. Nothing here is built yet.

- Phase 8 split into 8a–8b, one line each:
  - 8a: Undo and redo the review's edits in the files they changed.
  - 8b: Write the review's files with `:w`.
- 8a goes first. It's the riskier half, and 8b logs the save's edits as 8a's steps.
- Steps are logged where the files' changes are committed: `commit_files` for forwarded edits, and `:review-restore`, which commits each file at once and logs them all as one step.
  A commit that changes nothing logs nothing, like an accepted completion's replace by the same text (6.12). So two steps never name the same revision.
- A step's revision after is a child of the one before, so an undo that passes the check lands on the one before.
- `History` keeps a revision's last child private and isn't `Clone`, so a redo can't be checked before it runs. It runs, and is undone again if it landed on another revision.
- `Document::undo` takes a view, and the stream's view has a selection in every file the review edits: `apply_to_file` gives it one, and so does `ret`, which shows the file in that same view.
  The undo puts back that view's selection in the file, which `apply_to_file` set at the step's first change. That's where the stream's cursor goes.
- The sync at the start of `handle_event` forwards and commits before `u` runs, so a step from the key before is in the log by then.
- 8b's format step can be tested without a language server: `test_syntax_loader` takes language overrides, so a test can set an external formatter.
- Each file in the copy of `write_all_impl` picks its view as upstream does, with `get_synced_view_id`. That's the stream's view for any file the review edited.

### 6.17 Phase 8a: found

- `apply_to_file` became `edit_file`, which also queues the file for the next commit. `:review-restore` goes through it, so a restore commits at the next sync, as one step, instead of at once.
- A file has moved on when its revision isn't the step's, or it has changes not yet committed. `u` in the file's own view that brings it back to the step's revision lets the review's `u` go ahead. A test covers both.
- The messages: `<file> changed after this edit, so nothing was undone`, `<file> changed after the undo, so nothing was redone`, and `<file> is closed, so nothing was undone`.
- With a count, the cursors go where the last step walked put them. Undo pushes no jump, as upstream's doesn't.
- The commit before an undo from insert mode has no test. Upstream binds no undo key in insert mode.
- In the tests, `gg` in a file pushes a jump, so one `C-o` stays in the file. The tests edit there with `A`.

### 6.18 Phase 8b: found

- A staged review refuses `:w` with its read-only message. Its rows show the index, and the save's edits would be steps it can't undo. Without the refusal, `write_impl` would trim the stream and fail on its missing path.
- The copy trims every file before it saves any, where `write_all_impl` trims and saves one file at a time. A failed save returns through `?`, and the trims have to be logged first, as one step.
- `make_format_callback` always returns `Callback::Editor`. The wrapper reads the file's revision around it, so a format discarded because the file changed logs nothing.
- Helix prefixes the error: `'write': The review writes each file to its own path`. With nothing to write, the status says `No file in the review has unsaved changes`.
- The format test sets `tr` as an external formatter through `test_syntax_loader`'s overrides, so it runs on Unix only. Turning off either log call fails its test.
- `test_key_sequence` closes the app when it's done, so a `run_until` after it finds no view. The save test waits with `run_until` for both steps.

### 6.19 Phase 9: the split

Planned 2026-10-18, from a read of upstream's LSP code. Nothing here is built yet.

- Phase 9 split into 9a–9d, one line each:
  - 9a: Send goto, references, hover, and code actions from the review to the file's language server.
  - 9b: Show the files' diagnostics on the review's rows.
  - 9c: Rename symbols from the review.
  - 9d: a spike on completion and signature help, after the human has used 9a–9c.
- The lend goes first. It's the riskier of the three, and none of them depends on another.
  In the discussion before this plan, diagnostics were 9a. They swapped places for that rule.
- The lend, checked in `commands/lsp.rs`:
  - `goto_single_impl`, `goto_reference`, `hover`, and `code_action` read `current!` or `current_ref!` only before their request goes out.
  - Their answers use only the locations, the hovers, or actions bound to a server id. `jump_to_location` pushes the jump from the current view, and an `Action` runs on the `Editor` with its server.
  - `rename_symbol`'s prompt calls `current!` on confirm. `select_references_to_symbol_under_cursor` sets the answer as the selection of `current!`. `workspace_symbol_picker`'s query callback reads `doc!(editor)` on each query.
  - The plan had rejected switching the view (3.10). That holds for completion and for these three, not for the rest.
- `MappableCommand::Static` is a public variant, so the keymap overlay can build the lending commands without command-table rows.
- Only `gutter.rs`'s `diagnostic` filters diagnostics by the buffer's servers. Inline diagnostics and `]d` read the buffer's diagnostics as they are.
- `handle_lsp_diagnostics` dispatches `DiagnosticsDidChange` only for an open buffer. Reports for other files go into `Editor::diagnostics`, by URI, with no event.
  `Diagnostic` has no `PartialEq`, but `lsp::Diagnostic` has, so the check compares the reports.
  `Document::lsp_diagnostic_to_diagnostic` converts a report for a file that isn't open, with the file's text and its server's offset encoding.
- Language servers start when `Editor::open` opens a buffer, and `language_servers_with_feature` lists only initialized ones. That's why the first lent key can find none.
- Completion: `request_completions` reads `current_ref!` and takes its savepoint there. `ui/completion.rs` converts item ranges against the focused buffer's text.
- No `handlers.rs` seam after all. The plan's `DiagnosticsDidChange` hook would miss reports for files that aren't open.

### 6.20 Phase 9a: found

- The lend is two parts. `helix-view/src/review/lend.rs` maps the stream's primary selection into the file and loads its buffer. `commands/review/lsp.rs` points the view at the buffer, runs upstream's command, and points it back.
- The keymap reaches the lending commands through `commands::review`, which is now `pub(crate)`. That changes only the fork's own `mod` line in `commands.rs`.
- The selection carries over whole when both its ends are on one file's rows, so a code action asks about the range. Otherwise it's the cursor alone, which is all goto and hover read.
- A file that loads with text other than what the review read is refused, as an edit is (3.9), and the review then follows its buffer.
- Setting the file's selection for the stream's view fires `SelectionDidChange`. Its handlers, document highlights, code action hints, and signature help, work by buffer and view id and don't assume the view shows the buffer.
- The tests have no language server. They check the file is loaded, the selection lands at its place in the file, the view shows the stream again, and the refusals.
  Each lent command then gives upstream's answer for no server, which shows it ran. Whether a server's answer comes back right is for the manual check.
- A server that's still starting can't be tested without one. The check reads the buffer's servers, crate-private to `helix-view`, where `lend.rs` lives.

### 6.21 Phase 9b: found

- Placing runs at the end of every relayout, and from the sync when a file's report in `Editor::diagnostics` differs from the one last placed.
  The stream keeps the reports it placed, by path, to compare.
- An open buffer's diagnostics are placed only while the rows come from its current version. One that moved waits for the relayout that follows it.
- Helix maps the stream's own diagnostics through each edit to the stream, which is right for an edit made in the review.
  It isn't when the review follows an edit made in the file's own view: the relayout replaces the row's text, and the diagnostic with it. Placing them again then is what keeps them on their text, and the test edits in the file to show it.
- A range whose end is on a line the review doesn't show ends with its first row. A diagnostic on a hidden line, or on a file over the size caps, isn't shown.
- A report outlives the server that sent it, so one with no server left is read in UTF-16, the protocol's default. That's also what lets the test publish a report with no server.
- The gutter's dot is a copy of upstream's without the filter by the buffer's own servers, in `review/gutter.rs`. Tests can't see it. It's for the manual check.
- A staged review shows no diagnostics: its rows are the index, and the servers report on the file.

### 6.22 Phase 9c: found

- The rename borrows 9a's `review::lend` to load the file and put the selection at its place, then opens its own prompt.
  It takes the server and the position when the prompt opens. On confirm it checks the file's version, so an edit while the prompt was open refuses the rename.
- The rename is a step of the review's undo log, as a `:review-restore` is. `apply_workspace_edit` commits one revision per file, so the step is every buffer whose revision moved, and every buffer the edit opened, from revision 0.
  The plan didn't ask for that. Without it, `u` in the review would stop at the first file the rename touched.
- A code action's edit isn't a step. It lands from upstream's menu, after the key, where the review can't see it. `u` in the review stops at its files, as at any edit made elsewhere.
- Tests have no language server. The one test shows the swap is in place: on a removed row, `Space r` gives the review's refusal, which upstream's rename doesn't have. The prompt and the rename itself are for the manual check.

### 6.23 The manual check: found

Checked 2026-10-19, against a checklist and a `setup.sh` repo outside the checkout, with one scenario per file kind and scope.

- A scripted first pass found seven mismatches. After the fixes, the integration harness replayed every step it can see, 173 checks, language servers included. The human checked what only the screen shows: the gutter, colors, the picker, popups, and the untrusted refusal. Both passed.
- Fixed:
  - `all` listed a file renamed with `git mv`, then edited, as modified, with every line added. `visible_changes` kept the unstaged entry. An unstaged modification now keeps the staged entry, which says what the file is against `HEAD`. In `changed-files-views`, since the picker had it too.
  - `:stage-hunk` on a conflicted file said only its path. Staging wrapped the reason in a context, and a typable command shows only the outermost message.
  - A saved file the review loaded stayed open after the review closed, when it was the only other buffer. Helix gives the closed review's view to another buffer before the sync runs, so a view showing a file then said nothing about the user (6.15).
- Upstream behavior, left as it is:
  - Helix clears the status line when a language server's progress report ends, so `Restored 1 change` can vanish when the restore starts rust-analyzer.
  - Any key clears the status line, so a refused edit's message shows only until the next key.
  - Right after `]d`, `[d` selects the same diagnostic backward. The cursor is at its end.
- The review's rename fills in a selection longer than one character, as upstream does when it has no `prepareRename` answer. After a search, `;` first gives the word under the cursor.
- The harness can drive the review end to end, rust-analyzer included, with the working directory set to a fixture repo and one `Application` per test, run one at a time. A second `Application` in one test registers its hooks twice. The changed-files picker can't run there: its matcher thread panics outside the test's runtime.

### 6.24 Phase 9d: found

Spiked 2026-10-19 as a prototype on the local branch `spike/review-completion` (`e2c4bd45b`), with an end-to-end test against rust-analyzer 1.90. Nothing of it is on `fork`.

- It works, with 7 seams inside upstream functions, in four files upstream changed 0–1 times in the year. Every seam is added lines. No upstream line changes, and none is in a hot file. Two `mod` lines and one import come on top.
  Two of the seams are optional: the trigger characters (3.10).
- What the test showed, typing in a new row of a review against `HEAD`:
  - `s.gr`, `C-n`, `ret` puts `s.grow(by);`, from rust-analyzer's snippet, in the file once, and the review shows it.
  - An item from another module puts `use crate::shapes::Square;` at the file's top, inside a gap, and the review shows it as an added row. `u` in the review then takes out the item and the import as one step.
  - `.` and `(` reach the server as trigger characters. Typing into an open menu asks again for rust-analyzer's incomplete list, at the right place in the file.
  - `(` gets `fn grow(&mut self, by: f64)` from the file's server.
  - The 43 review integration tests and the 27 unit tests still pass with the prototype.
- The order of an item and its other edits matters. Made first, the other edits would move the file's version, and the sync would refuse the item as an edit to a file that changed.
  So they wait on the stream until the next sync has forwarded the item, then go to the file through `edit_file`, into the same undo step.
- Found on the way:
  - Rewriting an item's line numbers when the answer arrives doesn't hold. rust-analyzer resolves an item by computing it again from its `data`, so the resolved item has the file's line numbers.
    In the first run, that put `Square` on the file's last line: the row with the cursor line's number in the file. The review's checks let it through, since that row is editable. The menu now moves the item when it uses it, after any resolve.
  - The lend set the file's selection even when it was already there. `set_selection` fires `SelectionDidChange`, which asks signature help again while it's open, which lends again: a request every 120 ms.
    `set_selection` widens a point to a char, so the check compares the widened selection.
  - A lend from a job, as a completion request's, can run before the sync has forwarded the last edit, when the rows don't show the stream's text. The lend refuses then. 9a's commands never meet it, since the sync at the start of `handle_event` runs first.
  - The re-request for an incomplete list runs inside the key that typed the char, before the frame's sync. It runs the sync itself, held as under a menu, then lends.
- Left as upstream has it:
  - A preview reaches the file's server. Upstream keeps a preview from the server as a ghost transaction. The sync forwards it as an ordinary edit, so the file's buffer and its server stay in step, as with word completion since 7a.
  - The docs popup and the signature popup aren't highlighted. Both read the focused buffer's language, which is the stream's, and it has none. One seam each would fix it (8).
  - With several cursors, an item goes in at every cursor, as upstream does. A cursor on another file's row edits that file.
- Not tested:
  - Snippet tabstops live in the stream's buffer and map through relayouts. The test accepted a snippet but didn't `Tab` through it.
  - An item whose edit reaches a line the review doesn't show goes in at the cursor, over the word before it. rust-analyzer sent none.
  - The first trigger character typed in a file that isn't open triggers nothing. The file loads at the sync after the key, and its server starts then.
  - Signature help lends when you enter insert mode on a row, so it loads the file then, not at the first edit.
- The end-to-end test needs rust-analyzer, and the fixture as the working directory for the server's root. So it can't run in the parallel suite (`agents/TESTING.md`). The log line in `show_signature_help` is its only view of the popup.

### 6.25 Phase 9e: found

- Built from the spike's prototype, with its 7 seams, all added lines. The import in `ui/completion.rs` went: the seams name `helix_view::review` in full, as the other seams do.
- Changed from the prototype:
  - An edit over several lines moves only when its lines are on rows one after another. The prototype moved one across a removed row too, and the sync would have refused it. Now it goes in at the cursor, as one over a hidden line does.
  - An accepted item's other edits leave it even when the cursor isn't on a file's row. Left on the item, upstream would make them in the stream, at the file's line numbers.
  - The lend's check that the selection moved waits for 9f. Only signature help's hook asks again on a new selection.
- Tests:
  - A unit test moves edits onto rows laid out by hand: one row, two, both ranges of an insert-and-replace edit, another file's rows, and the edits that don't move.
  - Two integration tests play the menu's part with a hand-built item: `review::accept_item`, then the item's edit made in the stream, as the accept arm does. The item's import reaches the file after the item and shows as a row, and `u` takes out both. An item the sync puts back takes its import with it.
    Each fails with the bug it guards against, checked by hand: the import made before the item, or made though the item was put back.
  - `fork_review/rust_analyzer.rs` is the spike's test against rust-analyzer, ignored, so the suite, `land.sh`, and upstream's CI skip it. It's the repository's first ignored test.
    It waits for the incomplete list's answer by its new savepoint, not a sleep, so it shows the server answered the re-request. It passes in about 12 s against rust-analyzer 1.90.
- Not covered yet, as in 6.24: snippet tabstops, an item whose edit reaches a hidden line, and the first trigger character in a file that isn't open.

## 7. Risks

| Risk | Where | Mitigation |
| --- | --- | --- |
| Cursor jumps when rows appear or vanish | Reconcile, phase 2 | Match rows by identity. Unit tests on layout transitions: context → modified, modified → context, hunks merging and splitting. |
| An edit slips past the read-only guard | Guard, phase 1 | The check runs before every frame and compares the buffer's version, so any edit is put back before it's drawn. |
| Stream and source drift apart | Sync, phase 7 | One exact changeset per sync, from the stream's history. A file that moved under an edit refuses it. Debug builds assert after each sync that every editable row equals its source line. |
| A completion reverts a relayout into the file | Completion in the stream, phase 7a | Hold the reconcile while a completion menu is open. A test selects a word completion on an edited line, and the file gets the word once. |
| Undo surprises | Stream log, phase 8a | Undo only when every file's revision matches the step. Otherwise undo nothing and name the file. |
| A save's edits stop `u` in the review | `:w`, phase 8b | The review's `:w` logs its trims and formats as steps. Code actions on save aren't logged. |
| The copied save chain falls behind upstream's | `:w`, phase 8b | The ledger lists the copy with `typed.rs`, which a rebase reads for each entry. |
| An edit refused halfway through a command | Checks, phase 7 | Revert the whole window, explain in the status line. `c` over a removed row reverts the delete, and leaves insert mode, as phase 1 does. |
| Slow big reviews | Refresh, reconcile | Re-diff only stale files. Size caps per file. Load sources only when edited. Each keystroke relays out the stream, so 7a measures one. |
| Upstream churn | `ui/editor.rs` (9/yr), `commands.rs` (39/yr) | Seams stay one line. Bodies live in fork files. |
| The lend leaves the view on the file | Phase 9a | The view goes back as soon as upstream's command returns, with nothing between. A test checks the view and its selection after a lent command. |
| Upstream starts reading the focused buffer in an answer | Phase 9a, on a rebase | The ledger lists the lent commands. A rebase that touches `commands/lsp.rs` rereads them. |
| Completion needs seams inside upstream functions | Phases 9e–9f | The spike found 7, all added lines, in files upstream changed 0–1 times a year. The ledger names the function each sits in. Without them, the stream keeps word and path completion. |
| A resolved item lands on another file's row | Phase 9e | Items move onto the rows where the menu uses them, after any resolve. A unit test keeps an edit on a line the review doesn't show off the rows. |

## 8. Open decisions

None.

Decided, 2026-10-20, from the 9d spike (6.24):

- **Completion and signature help get built**, as 9e then 9f. 9e is the riskier, and 9f needs only the lend.
- **Both trigger-character seams go in.** Without them, `.` opens no menu until two word chars follow, and `(` opens no signature help.
- **Tests:** tests without a language server for moving an item onto the rows and for its other edits, plus the spike's end-to-end test against rust-analyzer, kept out of the suite and run again at the manual check.
  No fake language server, a test script that would answer completion and signature help. Nothing like it exists upstream.
- **The popups stay plain.** Two more seams, in `Completion::render` and `show_signature_help`, would highlight the docs and the signature in the file's language.

Decided, 2026-10-19:

- **`:review` back to an open review says its scope**, `Reviewing <scope> changes`, as a read does (6.7). Nothing else on screen names the scope.
- **A refusal's message is the one for the row the edit starts from.** Typing at column 1 of a header, a gap, or a removed row says it can't be edited, though the text would have joined the run above. `Backspace` at column 1 of an editable row says it would join, though it deletes the line ending of the row above.
  The rule reads the edit, not the cursor, so deleting a removed row's own line ending, the same edit, says join too.
- **A line whose every word changed gets no changed-word marks**, on either side. Its row's tint already says it's new, and the marks painted the whole row. A hunk that only adds or removes lines already had none.
- **The rule after a header takes only `ui.window`'s color**: its foreground, or its background when it has no foreground. 57 of the 135 bundled themes that set `ui.window` give it a background, which banded the header row.
- **The changed-file lists come out of `visible_changes` in path order**, so the picker's `all` list matches the review's, which no longer sorts on its own. When the status walk ends on the `all` list, the picker rebuilds it, since the live list kept each path's first entry. That's in `changed-files-views`.
- **Opening a gap leaves a cursor at the start of its first shown line.** The report saw the last line: the cursor was already on the first, but a selection on the gap row, as `/unchanged` leaves, stretched over every line shown, and a following `d` would delete them all.

Decided, 2026-10-18:

- **LSP scope:** diagnostics, goto, references, hover, code actions, and rename, as 9a–9c. Completion and signature help wait for a spike, 9d.
- **Diagnostics of files that aren't open** show too, from what their servers reported.
- **The diagnostics gutter gets a stream branch**, so the stream shows the dot.
- **A lent command loads its file on the first key.** Nothing is preloaded, so the first key can find the server still starting.

- **Relayout is live**, as you type, like Zed. Not deferred until `Esc`.
- **Undo when a file moved on** stops, undoes nothing, and names the file. It doesn't undo the file's own latest change.
- **`u` in the review undoes `:review-restore`** too, as one step.
- **`:w` in the review writes every file it shows with unsaved changes**, not only the ones the review edited, as Zed's project diff saves.
- **The save's own edits are steps in the log**, so `u` after `:w` undoes a format or a final newline first, as in a file.
- **No seams for `:x` and `:update`.** In the review they write nothing, and the book says to use `:w`.

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
- **Staging takes whole hunks**: every hunk the selection touches, not single lines.
- **`:review staged` stays**, read-only, as HEAD against the index.
- **The changed-files picker gets a new key** to jump into the review. `Enter` keeps opening the file.
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
