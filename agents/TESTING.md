# Testing

Which tests to write for a change, where they go, and how to run them.
`agents/VERIFY.md` covers what tests can't show.

## The survival question

Every test has to survive one question: would it fail if the behavior were wrong, or only if the code changed?
A test that only notices change is noise. Delete it, or don't write it.

Before writing the assertion, put a plausible bug into the code in your head: an inverted comparison, a dropped guard, an off-by-one at a line end.
If the test would still pass, redesign it.

## When tests are required

- A behavior change: a command, a picker action, a config key, a git status walk.
- A bug fix. Add the failing case first.
- A change that breaks a test updates the test in the same commit.

Pure wiring needs no test: a command-table row, a `mod` line, a re-export, or a moved body whose tests moved with it.

## Layers

| Layer | Owns | Where |
| --- | --- | --- |
| Unit | Logic that needs no running editor: config merging, git status, list filtering, row math | `#[cfg(test)] mod tests` in the file that holds the logic. Git fixtures go in `helix-vcs/src/git/test.rs`. |
| Integration | What keys do to the editor: commands, picker actions, what ends up open, selected, or yanked | The file in `helix-term/tests/test/` for the area, or a new `<area>.rs` there, plus its `mod` line in `helix-term/tests/integration.rs` |

## One home per behavior

A behavior's home is the lowest layer that can see it fail.
Don't test a rule again through keys when a unit test already pins it.
Add an integration test for what a unit test can't see: key routing, what a picker opens, the editor's state after a command.
If you add a second test of one rule anyway, put one `// WHY:` line above it.

## Write tests for

- Boundaries: the last line, a line with no trailing newline, an empty file, a count of zero.
- Both sides of a threshold.
- Config: the default, an override, and an unbind (`nop`), parsed from TOML written in the test.
- Git state: build the repository the case needs, with staged, unstaged, untracked, renamed, deleted, or conflicted files.
- Error paths: set up the condition and assert the exact message.
- Async work: force the order under test. Never sleep to order events.

## Banned

- Tests of the language or a library: constructors, getters, a `Default` that equals itself.
- A constant or default asserted against itself.
  To pin a default users rely on, compare it with a literal written in the test.
- Inventories: a command exists, or a key is in the table, with no behavior behind the assertion.
- Tests whose logic lives in the test, so they only check themselves.
- Many tests that differ only in an input. Write one table-driven test.

A test's name says what its assertion checks.

When a change already edits a test file, delete that file's tests that fail the survival question or are banned, in the same commit.
Upstream's tests are the fork's own. A change that makes one wrong updates or deletes it.

## Integration harness

`helix-term/tests/test/helpers.rs` has the harness.

- `test_key_sequence`, `test_key_sequences`, and `test_with_config` run keys against an `Application`.
  A sequence can take a closure that asserts on the app afterwards.
  `AppBuilder` builds the app with a file, a config, or input text.
- Only `Application::editor` is public. Assert what the editor holds: `documents()`, the selection, registers, `get_status()`.
  A picker's own state isn't reachable, so assert what its actions do.
- Each sequence runs until the editor goes idle, `idle-timeout` (250 ms) after the last key.
  Background work, like a file walk or a git status walk, can still be running then. Run `run_event_loop_until_idle` until it has finished.
- Reaching idle clears the editor's idle timer.
  A second `run_event_loop_until_idle` with no new events never returns, so call `app.editor.reset_idle_timer()` before it.
- Tests run in parallel in one process, so none can change the working directory.
  The changed-files picker walks the working directory, so its keys have no end-to-end test. Test the walk in `helix-vcs` instead.
- Build a git fixture in a temporary directory, as `empty_git_repo` and `create_commit` in `helix-vcs/src/git/test.rs` do.

`splits.rs` shows the closure style.
`config.rs`'s `picker_keys_overlay_defaults_and_nop_unbinds` and `git/test.rs`'s `changed_files_include_staged_and_unstaged` are the fork's unit examples.

## Running

Run the commands for the crates the change touched:

- `cargo test -p helix-vcs [filter]`, `cargo test -p helix-view [filter]`
- `cargo test -p helix-term --lib [filter]`
- `cargo integration-test -- [filter]`. It's a cargo alias that turns on the `integration` feature.
  `HELIX_LOG_LEVEL=debug` adds logs.

`tools/fork/land.sh --check` runs every check in the workspace. It's the gate, not the edit loop.
