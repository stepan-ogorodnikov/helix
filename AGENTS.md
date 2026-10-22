# Fork

This is a hard fork of Helix. It no longer rebases onto upstream, so upstream's code is the fork's own. Change it where the work needs it, its defaults and behavior included. A config key is for a choice the human wants to keep, not for keeping upstream's behavior.

Put code where it reads best. A large feature still gets its own module, as the review does in `helix-view/src/review/` and `helix-term/src/commands/review/`. Update the call sites and delete the old code instead of keeping a shim.

Some fork code still has the soft fork's shapes: thin wrappers in `commands.rs` and `typed.rs` with the body in a module of its own, and copies of upstream code instead of a shared function. Fold one back only when a change has its own reason to touch it.

## Git

`main` is the fork's line. Commit there. `upstream/master` is upstream's.

Never push to `helix-editor/helix`, and never open or comment on its pull requests and issues.

Push `main` only through `tools/fork/land.sh`, and only when the human asks. It runs the checks and fast-forwards `origin/main`. `tools/fork/land.sh --check` runs the same checks and pushes nothing. Nothing force-pushes `origin/main`.

Take a change from upstream only when the human asks, by `git cherry-pick -x`.

In a fork clone, `gh` picks the parent repository until `gh repo set-default stepan-ogorodnikov/helix` has run. Check `gh repo set-default --view` before using `gh`.

## Commits

Write the subject in the imperative and in sentence case, end it with a period, and leave out upstream's `area:` prefix. Say why in the body.

A commit does one thing, so it can be read and reverted alone.

The `pre-commit` hook in `tools/fork/hooks` formats and runs clippy. Fix what it reports instead of committing with `--no-verify`.

## Ledger

`FORK.md` lists every fork feature: what it touches upstream, and when to drop it. Update a feature's entry in the same commit as the feature.

## Plans

A feature that takes more than one commit gets a plan in `fork/<feature>.md`. A small fix doesn't.

A plan whose phases are all done is history. The rules it quotes may be older than these.

Ask the human every question whose answer changes the plan, like scope, order, or approach, instead of guessing. Commit the plan before the code. Implement only after the human approves the plan and asks for the work.

Each phase is one commit that builds and passes its tests. Order the phases so each starts from what the previous one left, and put the riskiest as early as its dependencies allow. If a phase's subject can't say what it does in one line, split it. Book pages ride with the phase that makes them true.

Each phase's commit marks the phase done in the plan and records what the work found.

## Guides

- `agents/HELIX.md`: upstream's docs and checks, where code lives, comment tags. Read it before changing Rust code.
- `agents/TESTING.md`: which tests to write, where they go, and how to run them. Read it before writing or changing a test.
- `agents/REVIEW.md`: how to review a diff on `main`. Read it for any review.
- `agents/REBASE.md`: moving the stack onto a new upstream.
- `agents/VERIFY.md`: how to hand a user-visible change back for a check. Never run `hx` interactively or drive it through a terminal.
